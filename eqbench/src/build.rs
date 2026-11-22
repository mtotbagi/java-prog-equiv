//! Compilation of the cases to bytecode with `javac`, then to JSON with
//! `jvm2json` (<https://github.com/kalhauge/jvm2json>).
//!
//! The dataset is never modified. Sources are first copied to a staging
//! directory where they are fixed: the `package` declaration must match the
//! path of the case (a few files declare the package of a sibling case) and
//! the public class must match the file name (`oldV` vs `newV`). Old and new
//! versions are compiled separately because some of them define the same
//! auxiliary classes (e.g. `raytrace`).
//!
//! Layout of the output directory:
//! ```text
//! staging/<old|new>/benchmarks/<bench>/<prog>/<Eq|Neq>/<oldV|newV>.java
//! classes/<old|new>/benchmarks/<bench>/<prog>/<Eq|Neq>/*.class
//! cases/<case-id>/<old|new>/{<oldV|newV>.java,*.json}
//! cases/<case-id>/miter/{src,classes}/   or   cases/<case-id>/no-miter.txt
//! ```
//!
//! `cases` is what the analysis tools get: the fixed source and the JSON of
//! every class of each version, and the miter comparing them (see
//! [`crate::miter`]) or why there is none.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::case::{Case, CaseId, Dataset, Version};
use crate::miter::{self, NEW_PACKAGE, OLD_PACKAGE};

/// Paths of the build artifacts.
#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Layout { root: root.into() }
    }

    pub fn staging(&self, version: Version) -> PathBuf {
        self.root.join("staging").join(version.as_str())
    }

    pub fn classes(&self, version: Version) -> PathBuf {
        self.root.join("classes").join(version.as_str())
    }

    /// Directory holding the source and the JSON of every class of one
    /// version of a case.
    pub fn case_dir(&self, id: &CaseId, version: Version) -> PathBuf {
        self.root
            .join("cases")
            .join(id.to_string())
            .join(version.as_str())
    }

    /// Directory of the miter of a case, with its sources and classes.
    pub fn miter_dir(&self, id: &CaseId) -> PathBuf {
        self.root.join("cases").join(id.to_string()).join("miter")
    }

    /// Why a case has no miter.
    pub fn no_miter_file(&self, id: &CaseId) -> PathBuf {
        self.root
            .join("cases")
            .join(id.to_string())
            .join("no-miter.txt")
    }
}

/// Something that went wrong for one case, the build goes on.
#[derive(Debug)]
pub struct Failure {
    pub what: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub sources: usize,
    /// Sources whose package or class name had to be fixed.
    pub fixed_sources: usize,
    pub compile_failures: Vec<Failure>,
    pub classes: usize,
    pub decompile_failures: Vec<Failure>,
    pub miters: usize,
    /// Cases whose method the miter cannot compare.
    pub no_miter: usize,
    pub miter_failures: Vec<Failure>,
    pub elapsed: Duration,
}

/// Rebuilds everything from scratch.
pub fn build(dataset: &Dataset, layout: &Layout) -> Result<Report> {
    let start = Instant::now();
    check_tool("javac", "-version")?;
    check_tool("jvm2json", "--help")?;
    clean(layout)?;

    let mut report = Report::default();
    for version in Version::ALL {
        let staged = stage(dataset, layout, version, &mut report)?;
        eprintln!("compiling {} {version} sources", staged.len());
        compile(&staged, &layout.classes(version), version, &mut report)?;
    }
    decompile(dataset, layout, &mut report)?;
    miters(dataset, layout, &mut report)?;
    report.elapsed = start.elapsed();
    Ok(report)
}

/// Removes every build artifact, returns whether there was anything to remove.
pub fn clean(layout: &Layout) -> Result<bool> {
    if !layout.root.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&layout.root)
        .with_context(|| format!("cannot remove {}", layout.root.display()))?;
    Ok(true)
}

/// Checks that `tool` can be run, `/usr/bin/javac` on macOS exists even
/// without a JDK.
fn check_tool(tool: &str, arg: &str) -> Result<()> {
    let ok = Command::new(tool)
        .arg(arg)
        .output()
        .is_ok_and(|out| out.status.success());
    if !ok {
        bail!("`{tool}` is not available, enter the dev shell first (`just dev-env`)");
    }
    Ok(())
}

/// A staged source, ready to be compiled.
struct Staged<'a> {
    case: &'a Case,
    path: PathBuf,
}

fn stage<'a>(
    dataset: &'a Dataset,
    layout: &Layout,
    version: Version,
    report: &mut Report,
) -> Result<Vec<Staged<'a>>> {
    let root = layout.staging(version);
    let mut staged = Vec::with_capacity(dataset.cases.len());
    for case in &dataset.cases {
        let source = case.source(version);
        let text = fs::read_to_string(source)
            .with_context(|| format!("cannot read {}", source.display()))?;
        let class = source
            .file_stem()
            .and_then(|s| s.to_str())
            .context("source without a file name")?;
        let (fixed, package_changed) = fix_package(&text, &case.id.java_package());
        let (fixed, class_changed) = fix_class_name(&fixed, class);
        if package_changed || class_changed {
            report.fixed_sources += 1;
        }
        let dir = root.join(case.id.package_dir());
        fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let path = dir.join(source.file_name().context("source without a file name")?);
        fs::write(&path, fixed).with_context(|| format!("cannot write {}", path.display()))?;
        staged.push(Staged { case, path });
    }
    report.sources += staged.len();
    Ok(staged)
}

/// Rewrites the `package` declaration of `source` to `package`.
/// Returns the new source and whether it changed.
pub fn fix_package(source: &str, package: &str) -> (String, bool) {
    let declaration = format!("package {package};");
    let mut out = String::with_capacity(source.len() + declaration.len());
    let mut found = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !found
            && trimmed.starts_with("package")
            && trimmed["package".len()..].starts_with(char::is_whitespace)
            && let Some(end) = line.find(';')
        {
            found = true;
            let indent = &line[..line.len() - trimmed.len()];
            out.push_str(indent);
            out.push_str(&declaration);
            out.push_str(&line[end + 1..]);
        } else {
            out.push_str(line);
        }
    }
    if !found {
        out = format!("{declaration}\n{source}");
    }
    let changed = out != source;
    (out, changed)
}

/// Renames the public `oldV`/`newV` class to `class`, the name of the file.
/// Returns the new source and whether it changed.
pub fn fix_class_name(source: &str, class: &str) -> (String, bool) {
    for wrong in ["oldV", "newV"] {
        if wrong == class {
            continue;
        }
        let declaration = format!("public class {wrong}");
        if let Some(at) = source.find(&declaration) {
            let end = at + declaration.len();
            if source[end..].starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let fixed = format!("{}public class {class}{}", &source[..at], &source[end..]);
            return (fixed, true);
        }
    }
    (source.to_string(), false)
}

fn javac(files: &[&Path], out: &Path) -> Result<std::process::Output> {
    Command::new("javac")
        .args(["-g", "-nowarn", "-encoding", "UTF-8", "-d"])
        .arg(out)
        .args(files)
        .output()
        .context("cannot run javac")
}

/// Compiles all the staged sources at once, falling back to one `javac`
/// per file to find the culprits when the batch fails.
fn compile(staged: &[Staged], out: &Path, version: Version, report: &mut Report) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("cannot create {}", out.display()))?;
    let files: Vec<&Path> = staged.iter().map(|s| s.path.as_path()).collect();
    if javac(&files, out)?.status.success() {
        return Ok(());
    }
    eprintln!("javac failed on the {version} batch, compiling file by file");
    for s in staged {
        let output = javac(&[&s.path], out)?;
        if !output.status.success() {
            report.compile_failures.push(Failure {
                what: format!("{} ({version})", s.case.id),
                message: first_lines(&String::from_utf8_lossy(&output.stderr), 3),
            });
        }
    }
    Ok(())
}

struct Job {
    what: String,
    class: PathBuf,
    json: PathBuf,
}

/// Fills the case directories: copies the staged sources, then runs
/// `jvm2json` on every compiled class, in parallel.
fn decompile(dataset: &Dataset, layout: &Layout, report: &mut Report) -> Result<()> {
    let mut jobs = Vec::new();
    for case in &dataset.cases {
        for version in Version::ALL {
            let classes = layout.classes(version).join(case.id.package_dir());
            // Missing when the case did not compile, already reported.
            let Ok(entries) = fs::read_dir(&classes) else {
                continue;
            };
            let out = layout.case_dir(&case.id, version);
            fs::create_dir_all(&out).with_context(|| format!("cannot create {}", out.display()))?;
            let file = case
                .source(version)
                .file_name()
                .context("source without a file name")?;
            let staged = layout
                .staging(version)
                .join(case.id.package_dir())
                .join(file);
            fs::copy(&staged, out.join(file))
                .with_context(|| format!("cannot copy {}", staged.display()))?;
            for entry in entries {
                let class = entry?.path();
                if class.extension().is_none_or(|e| e != "class") {
                    continue;
                }
                let name = class
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .context("class without a file name")?;
                jobs.push(Job {
                    what: format!("{} ({version}) {name}", case.id),
                    json: out.join(format!("{name}.json")),
                    class,
                });
            }
        }
    }

    eprintln!("decompiling {} classes with jvm2json", jobs.len());
    let failures = in_parallel(&jobs, |job| {
        jvm2json(job).err().map(|message| Failure {
            what: job.what.clone(),
            message,
        })
    });
    report.classes = jobs.len() - failures.len();
    report.decompile_failures = failures;
    Ok(())
}

/// Runs `job` on every item, one thread per core, and returns the failures.
fn in_parallel<T: Sync>(items: &[T], job: impl Fn(&T) -> Option<Failure> + Sync) -> Vec<Failure> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = thread::available_parallelism().map_or(4, |n| n.get());
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(failure) = job(item) {
                        failures.lock().expect("poisoned lock").push(failure);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("poisoned lock");
    failures.sort_by(|a, b| a.what.cmp(&b.what));
    failures
}

/// Writes and compiles the miter of every built case, or the reason why it
/// has none.
fn miters(dataset: &Dataset, layout: &Layout, report: &mut Report) -> Result<()> {
    let mut jobs = Vec::new();
    for case in &dataset.cases {
        if !Version::ALL
            .iter()
            .all(|v| layout.case_dir(&case.id, *v).is_dir())
        {
            continue;
        }
        match write_miter(case, layout)? {
            Ok(dir) => jobs.push((case, dir)),
            Err(reason) => {
                let file = layout.no_miter_file(&case.id);
                fs::write(&file, format!("{reason}\n"))
                    .with_context(|| format!("cannot write {}", file.display()))?;
                report.no_miter += 1;
            }
        }
    }

    eprintln!("compiling {} miters", jobs.len());
    let failures = in_parallel(&jobs, |(case, dir)| {
        let message = compile_miter(dir).err()?;
        // Tools see a case without a miter, and why.
        let _ = fs::remove_dir_all(dir);
        let _ = fs::write(
            layout.no_miter_file(&case.id),
            format!("the miter does not compile:\n{message}\n"),
        );
        Some(Failure {
            what: case.id.to_string(),
            message,
        })
    });
    report.miters = jobs.len() - failures.len();
    report.miter_failures = failures;
    Ok(())
}

/// Writes the sources of the miter of `case`. The inner error is why the case
/// has no miter.
fn write_miter(case: &Case, layout: &Layout) -> Result<Result<PathBuf, String>> {
    let method = case.desc.entry();
    let mut sources = Vec::new();
    let mut signatures = Vec::new();
    for (version, package) in [(Version::Old, OLD_PACKAGE), (Version::New, NEW_PACKAGE)] {
        let class = case
            .source(version)
            .file_stem()
            .and_then(|s| s.to_str())
            .context("source without a file name")?;
        let dir = layout.case_dir(&case.id, version);
        let json_file = dir.join(format!("{class}.json"));
        let Ok(json) = fs::read_to_string(&json_file) else {
            return Ok(Err(format!("no {class}.json")));
        };
        let json: miter::Class = serde_json::from_str(&json)
            .with_context(|| format!("invalid {}", json_file.display()))?;
        let signature = match miter::signature(&json, method) {
            Ok(signature) => signature,
            Err(reason) => return Ok(Err(format!("{class}: {reason}"))),
        };
        let source = fs::read_to_string(dir.join(format!("{class}.java")))?;
        let (source, _) = fix_package(&source, package);
        let Some(source) = miter::add_bridge(&source, class, method, &signature) else {
            return Ok(Err(format!("no class {class} in its source")));
        };
        sources.push((package, class, source));
        signatures.push(signature);
    }
    if signatures[0] != signatures[1] {
        return Ok(Err(format!("the signature of `{method}` changes")));
    }

    let dir = layout.miter_dir(&case.id);
    let src = dir.join("src");
    let mut files = vec![(
        "eqbench/Miter.java".to_string(),
        miter::miter_source(&signatures[0], sources[0].1, sources[1].1),
    )];
    files.push((
        "org/sosy_lab/sv_benchmarks/Verifier.java".to_string(),
        miter::VERIFIER_SOURCE.to_string(),
    ));
    for (package, class, source) in sources {
        files.push((
            format!("{}/{class}.java", package.replace('.', "/")),
            source,
        ));
    }
    for (path, content) in files {
        let path = src.join(path);
        fs::create_dir_all(path.parent().context("no parent")?)?;
        fs::write(&path, content).with_context(|| format!("cannot write {}", path.display()))?;
    }
    Ok(Ok(dir))
}

/// Compiles the miter for Java 8, which the JPF based tools need.
fn compile_miter(dir: &Path) -> Result<(), String> {
    let src = dir.join("src");
    let files = [
        "eqbench/Miter.java",
        "org/sosy_lab/sv_benchmarks/Verifier.java",
        "eqbench/v1",
        "eqbench/v2",
    ];
    let mut command = Command::new("javac");
    command
        .args([
            "--release",
            "8",
            "-g",
            "-nowarn",
            "-Xlint:-options",
            "-encoding",
            "UTF-8",
            "-d",
        ])
        .arg(dir.join("classes"));
    for file in files {
        let path = src.join(file);
        if path.is_dir() {
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                command.arg(entry.map_err(|e| e.to_string())?.path());
            }
        } else {
            command.arg(path);
        }
    }
    let output = command
        .output()
        .map_err(|e| format!("cannot run javac: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_lines(&String::from_utf8_lossy(&output.stderr), 3))
    }
}

fn jvm2json(job: &Job) -> Result<(), String> {
    let output = Command::new("jvm2json")
        .arg("-s")
        .arg(&job.class)
        .arg("-t")
        .arg(&job.json)
        .output()
        .map_err(|e| format!("cannot run jvm2json: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_lines(&String::from_utf8_lossy(&output.stderr), 3))
    }
}

fn first_lines(text: &str, n: usize) -> String {
    text.lines().take(n).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_a_wrong_package() {
        let src = "package benchmarks.a.b.Eq;\npublic class oldV {}\n";
        let (fixed, changed) = fix_package(src, "benchmarks.a.b.Neq");
        assert!(changed);
        assert_eq!(fixed, "package benchmarks.a.b.Neq;\npublic class oldV {}\n");
    }

    #[test]
    fn keeps_a_correct_package() {
        let src = "package benchmarks.a.b.Eq;\r\nclass X {}\r\n";
        let (fixed, changed) = fix_package(src, "benchmarks.a.b.Eq");
        assert!(!changed);
        assert_eq!(fixed, src);
    }

    #[test]
    fn keeps_trailing_content_and_ignores_lookalikes() {
        let src = "// packages\npackage  x.y ; // comment\nclass packageInfo {}\n";
        let (fixed, _) = fix_package(src, "a.b");
        assert_eq!(
            fixed,
            "// packages\npackage a.b; // comment\nclass packageInfo {}\n"
        );
    }

    #[test]
    fn fixes_a_wrong_class_name() {
        let src = "package p;\npublic class newV {\n}\n";
        assert_eq!(
            fix_class_name(src, "oldV"),
            ("package p;\npublic class oldV {\n}\n".to_string(), true)
        );
        assert!(!fix_class_name(src, "newV").1);
        assert!(!fix_class_name("public class newVersion {}", "oldV").1);
        assert!(!fix_class_name("class Sphere {}", "oldV").1);
    }

    #[test]
    fn adds_a_missing_package() {
        let (fixed, changed) = fix_package("class X {}\n", "a.b");
        assert!(changed);
        assert_eq!(fixed, "package a.b;\nclass X {}\n");
    }
}
