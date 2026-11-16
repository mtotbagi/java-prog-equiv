//! Runs an analysis tool on every case and compares its predictions with the
//! ground truth.
//!
//! The tool is called as `<cmd…> <old-dir> <new-dir> <method>`, the two
//! directories holding the source (`oldV.java` / `newV.java`) and the JSON
//! (jvm2json) of every class of the two versions of a case, and `<method>`
//! naming the method to compare (`method name` of `J-Desc.json`). It must print `eq`, `neq` or `unknown` on the first line
//! of its standard output.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::build::Layout;
use crate::case::{Case, CaseId, Kind, Version};

/// Time given to the tool for one case.
pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Predicted(Kind),
    /// The tool could not decide.
    Unknown,
    /// The first line of the output was neither `eq`, `neq` nor `unknown`.
    Invalid(String),
    /// The tool exited with a failure status (`None` when killed by a signal).
    Crashed(Option<i32>),
    Timeout,
    /// The case was not built, the tool was not run.
    NotBuilt,
}

impl Outcome {
    pub fn label(&self) -> String {
        match self {
            Outcome::Predicted(Kind::Eq) => "eq".to_string(),
            Outcome::Predicted(Kind::Neq) => "neq".to_string(),
            Outcome::Unknown => "unknown".to_string(),
            Outcome::Invalid(_) => "invalid".to_string(),
            Outcome::Crashed(Some(code)) => format!("exit {code}"),
            Outcome::Crashed(None) => "killed".to_string(),
            Outcome::Timeout => "timeout".to_string(),
            Outcome::NotBuilt => "not built".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub id: CaseId,
    pub outcome: Outcome,
    pub elapsed: Duration,
}

impl CaseResult {
    pub fn is_correct(&self) -> bool {
        self.outcome == Outcome::Predicted(self.id.kind)
    }
}

/// The command to run for `cmd`.
///
/// A tool given as a directory holding a `flake.nix` (`eqbench/tools/<tool>`)
/// is built once with nix, before any case, and the only program of its
/// default package is run instead, with the rest of `cmd` as arguments.
pub fn resolve_tool(cmd: &[String]) -> Result<Vec<String>> {
    let (first, rest) = cmd.split_first().context("no command given")?;
    if !Path::new(first).join("flake.nix").is_file() {
        return Ok(cmd.to_vec());
    }
    // A bare relative path would be read as a flake registry name.
    let flake = if first.starts_with('/') || first.starts_with('.') {
        first.clone()
    } else {
        format!("./{first}")
    };
    eprintln!("building {flake} with nix");
    let output = Command::new("nix")
        .args(["--extra-experimental-features", "nix-command flakes"])
        .args(["build", "--no-link", "--print-out-paths", &flake])
        .stderr(Stdio::inherit())
        .output()
        .context("cannot run nix")?;
    if !output.status.success() {
        bail!("cannot build {flake} with nix");
    }
    let stdout = String::from_utf8(output.stdout).context("nix printed a non UTF-8 path")?;
    let out = stdout
        .lines()
        .last()
        .context("nix printed no output path")?;
    let bin = Path::new(out.trim()).join("bin");
    let programs: Vec<PathBuf> = fs::read_dir(&bin)
        .with_context(|| format!("{flake} has no program, {} is missing", bin.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    let [program] = programs.as_slice() else {
        bail!("{} should hold exactly one program", bin.display());
    };
    let mut resolved = vec![program.to_string_lossy().into_owned()];
    resolved.extend(rest.iter().cloned());
    Ok(resolved)
}

/// Runs the tool on one case.
pub fn run_case(cmd: &[String], case: &Case, layout: &Layout) -> Result<CaseResult> {
    let old = layout.case_dir(&case.id, Version::Old);
    let new = layout.case_dir(&case.id, Version::New);
    let (outcome, elapsed) = if old.is_dir() && new.is_dir() {
        run_tool(cmd, &old, &new, &case.desc.method, TIMEOUT)?
    } else {
        (Outcome::NotBuilt, Duration::ZERO)
    };
    Ok(CaseResult {
        id: case.id.clone(),
        outcome,
        elapsed,
    })
}

/// Runs `cmd old new method`, killing it after `timeout`.
/// Fails only when the tool cannot be started at all.
pub fn run_tool(
    cmd: &[String],
    old: &Path,
    new: &Path,
    method: &str,
    timeout: Duration,
) -> Result<(Outcome, Duration)> {
    let (program, args) = cmd.split_first().context("no command given")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(old)
        .arg(new)
        .arg(method)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Own process group, so a timeout also kills what the tool started
    // (e.g. `uv run` spawning python).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);

    let start = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot run `{program}`"))?;

    // Read in the background so a chatty tool cannot block on a full pipe.
    let mut stdout = child.stdout.take().context("no stdout")?;
    let reader = thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            kill_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(1));
    };
    let elapsed = start.elapsed();

    let outcome = match status {
        // The reader is left behind: a leftover process may still hold the pipe.
        None => Outcome::Timeout,
        Some(status) if !status.success() => Outcome::Crashed(status.code()),
        Some(_) => parse_prediction(&reader.join().unwrap_or_default()),
    };
    Ok((outcome, elapsed))
}

/// Kills the process group led by `pid`.
fn kill_group(pid: u32) {
    if cfg!(unix) {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stderr(Stdio::null())
            .status();
    }
}

/// Reads `eq`, `neq` or `unknown` from the first non-empty line, ignoring case.
pub fn parse_prediction(output: &str) -> Outcome {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty());
    match line.map(str::to_lowercase).as_deref() {
        Some("eq") => Outcome::Predicted(Kind::Eq),
        Some("neq") => Outcome::Predicted(Kind::Neq),
        Some("unknown") => Outcome::Unknown,
        _ => Outcome::Invalid(line.unwrap_or_default().chars().take(40).collect()),
    }
}

/// Scores of one class (`eq` or `neq`) seen as the positive one.
///
/// A case without a prediction (unknown, timeout, crash, ...) is a false
/// negative for its own class and does not count against the precision of
/// either class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassScore {
    /// Cases of this class.
    pub actual: usize,
    /// Cases predicted as this class.
    pub predicted: usize,
    /// Cases of this class predicted as this class (true positives).
    pub correct: usize,
}

impl ClassScore {
    /// `None` when the tool never predicted this class.
    pub fn precision(&self) -> Option<f64> {
        (self.predicted > 0).then(|| self.correct as f64 / self.predicted as f64)
    }

    /// `None` when the dataset has no case of this class.
    pub fn recall(&self) -> Option<f64> {
        (self.actual > 0).then(|| self.correct as f64 / self.actual as f64)
    }

    /// `2·TP / (2·TP + FP + FN)`, 0 when there is nothing to score.
    pub fn f1(&self) -> f64 {
        let denominator = self.predicted + self.actual;
        if denominator == 0 {
            0.0
        } else {
            2.0 * self.correct as f64 / denominator as f64
        }
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub total: usize,
    pub correct: usize,
    pub eq: ClassScore,
    pub neq: ClassScore,
    pub total_time: Duration,
    pub slowest: Option<(CaseId, Duration)>,
    pub unknown: usize,
    pub timeouts: usize,
    pub crashes: usize,
    pub invalid: usize,
    pub not_built: usize,
}

impl Summary {
    pub fn new(results: &[CaseResult]) -> Self {
        let mut summary = Summary {
            total: results.len(),
            ..Summary::default()
        };
        for r in results {
            summary.correct += usize::from(r.is_correct());
            summary.class_mut(r.id.kind).actual += 1;
            if let Outcome::Predicted(kind) = r.outcome {
                let class = summary.class_mut(kind);
                class.predicted += 1;
                class.correct += usize::from(kind == r.id.kind);
            }
            summary.total_time += r.elapsed;
            if summary.slowest.as_ref().is_none_or(|(_, t)| r.elapsed > *t) {
                summary.slowest = Some((r.id.clone(), r.elapsed));
            }
            match r.outcome {
                Outcome::Predicted(_) => {}
                Outcome::Unknown => summary.unknown += 1,
                Outcome::Invalid(_) => summary.invalid += 1,
                Outcome::Crashed(_) => summary.crashes += 1,
                Outcome::Timeout => summary.timeouts += 1,
                Outcome::NotBuilt => summary.not_built += 1,
            }
        }
        summary
    }

    pub fn mean_time(&self) -> Duration {
        let ran = self.total - self.not_built;
        if ran == 0 {
            Duration::ZERO
        } else {
            self.total_time / ran as u32
        }
    }

    /// Correct predictions over the total number of cases, in percent.
    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            100.0 * self.correct as f64 / self.total as f64
        }
    }

    /// Mean of the F1 scores of `eq` and `neq`, so that neither class is
    /// favoured by the choice of a positive one.
    pub fn macro_f1(&self) -> f64 {
        (self.eq.f1() + self.neq.f1()) / 2.0
    }

    pub fn class(&self, kind: Kind) -> &ClassScore {
        match kind {
            Kind::Eq => &self.eq,
            Kind::Neq => &self.neq,
        }
    }

    fn class_mut(&mut self, kind: Kind) -> &mut ClassScore {
        match kind {
            Kind::Eq => &mut self.eq,
            Kind::Neq => &mut self.neq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, timeout: Duration) -> Outcome {
        let cmd = ["sh".to_string(), "-c".to_string(), script.to_string()];
        run_tool(&cmd, Path::new("old"), Path::new("new"), "f", timeout)
            .unwrap()
            .0
    }

    #[test]
    fn parses_predictions() {
        assert_eq!(parse_prediction("eq\n"), Outcome::Predicted(Kind::Eq));
        assert_eq!(
            parse_prediction("\n  NEQ \nmore"),
            Outcome::Predicted(Kind::Neq)
        );
        assert_eq!(parse_prediction("Unknown\n"), Outcome::Unknown);
        assert_eq!(parse_prediction("maybe"), Outcome::Invalid("maybe".into()));
        assert_eq!(parse_prediction(""), Outcome::Invalid(String::new()));
    }

    #[test]
    fn runs_the_tool_with_both_directories() {
        let second = Duration::from_secs(5);
        // With `sh -c`, the first extra argument is `$0`.
        assert_eq!(
            sh(r#"[ "$0 $1 $2" = "old new f" ] && echo neq"#, second),
            Outcome::Predicted(Kind::Neq)
        );
        assert_eq!(sh("exit 3", second), Outcome::Crashed(Some(3)));
        assert_eq!(sh("echo yes", second), Outcome::Invalid("yes".into()));
    }

    #[test]
    fn kills_slow_tools_and_their_children() {
        let start = Instant::now();
        // The subshell keeps stdout open after `sh` itself is killed.
        assert_eq!(
            sh("(sleep 5; echo eq) & wait", Duration::from_millis(100)),
            Outcome::Timeout
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    fn result(id: &str, outcome: Outcome) -> CaseResult {
        CaseResult {
            id: id.parse().unwrap(),
            outcome,
            elapsed: Duration::from_millis(10),
        }
    }

    #[test]
    fn scores_each_class() {
        let results = [
            result("a-a-Eq", Outcome::Predicted(Kind::Eq)),
            result("a-b-Eq", Outcome::Predicted(Kind::Neq)),
            result("a-c-Eq", Outcome::Timeout),
            result("a-d-Neq", Outcome::Predicted(Kind::Neq)),
            result("a-e-Neq", Outcome::Predicted(Kind::Eq)),
            result("a-f-Neq", Outcome::Unknown),
        ];
        let summary = Summary::new(&results);
        assert_eq!(summary.correct, 2);
        assert_eq!(summary.accuracy(), 100.0 * 2.0 / 6.0);
        assert_eq!(summary.timeouts, 1);
        assert_eq!(summary.unknown, 1);

        let eq = summary.eq;
        assert_eq!(
            eq,
            ClassScore {
                actual: 3,
                predicted: 2,
                correct: 1
            }
        );
        assert_eq!(eq.precision(), Some(0.5));
        assert_eq!(eq.recall(), Some(1.0 / 3.0));
        assert_eq!(eq.f1(), 0.4);

        // The unknown answer lowers the recall of `neq`, not its precision.
        let neq = summary.neq;
        assert_eq!(neq.precision(), Some(0.5));
        assert_eq!(neq.recall(), Some(1.0 / 3.0));
        assert_eq!(neq.f1(), 0.4);
        assert!((summary.macro_f1() - 0.4).abs() < 1e-12);
    }

    #[test]
    fn scores_a_tool_that_always_answers_the_same() {
        let results = [
            result("a-a-Eq", Outcome::Predicted(Kind::Eq)),
            result("a-b-Neq", Outcome::Predicted(Kind::Eq)),
        ];
        let summary = Summary::new(&results);
        assert_eq!(summary.neq.precision(), None);
        assert_eq!(summary.neq.recall(), Some(0.0));
        assert_eq!(summary.neq.f1(), 0.0);
        assert!((summary.eq.f1() - 2.0 / 3.0).abs() < 1e-12);
        assert_eq!(Summary::new(&[]).macro_f1(), 0.0);
    }

    #[test]
    fn keeps_commands_that_are_not_flakes() {
        let cmd = ["sh".to_string(), "-c".to_string(), "echo eq".to_string()];
        assert_eq!(resolve_tool(&cmd).unwrap(), cmd);
        assert!(resolve_tool(&[]).is_err());
    }

    #[test]
    fn fails_when_the_tool_does_not_exist() {
        let cmd = ["./does-not-exist".to_string()];
        assert!(run_tool(&cmd, Path::new("a"), Path::new("b"), "f", TIMEOUT).is_err());
    }
}
