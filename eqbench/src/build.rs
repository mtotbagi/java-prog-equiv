//! Compilation of the cases to bytecode with `javac`.
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
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::case::{Case, Dataset, Version};

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
    pub elapsed: Duration,
}

/// Rebuilds everything from scratch.
pub fn build(dataset: &Dataset, layout: &Layout) -> Result<Report> {
    let start = Instant::now();
    check_tool("javac", "-version")?;

    if layout.root.exists() {
        fs::remove_dir_all(&layout.root)
            .with_context(|| format!("cannot remove {}", layout.root.display()))?;
    }

    let mut report = Report::default();
    for version in Version::ALL {
        let staged = stage(dataset, layout, version, &mut report)?;
        eprintln!("compiling {} {version} sources", staged.len());
        compile(&staged, &layout.classes(version), version, &mut report)?;
    }
    report.elapsed = start.elapsed();
    Ok(report)
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
        assert_eq!(fixed, "// packages\npackage a.b; // comment\nclass packageInfo {}\n");
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
