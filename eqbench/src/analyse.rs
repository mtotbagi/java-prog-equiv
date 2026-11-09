//! Runs an analysis tool on every case and compares its predictions with the
//! ground truth.
//!
//! The tool is called as `<cmd…> <old-dir> <new-dir>`, the two directories
//! holding the JSON (jvm2json) of every class of the two versions of a case.
//! It must print `eq` or `neq` on the first line of its standard output.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::build::Layout;
use crate::case::{Case, CaseId, Kind, Version};

/// Time given to the tool for one case.
pub const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Predicted(Kind),
    /// The first line of the output was neither `eq` nor `neq`.
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

/// Runs the tool on one case.
pub fn run_case(cmd: &[String], case: &Case, layout: &Layout) -> Result<CaseResult> {
    let old = layout.decompiled(&case.id, Version::Old);
    let new = layout.decompiled(&case.id, Version::New);
    let (outcome, elapsed) = if old.is_dir() && new.is_dir() {
        run_tool(cmd, &old, &new, TIMEOUT)?
    } else {
        (Outcome::NotBuilt, Duration::ZERO)
    };
    Ok(CaseResult {
        id: case.id.clone(),
        outcome,
        elapsed,
    })
}

/// Runs `cmd old new`, killing it after `timeout`.
/// Fails only when the tool cannot be started at all.
pub fn run_tool(
    cmd: &[String],
    old: &Path,
    new: &Path,
    timeout: Duration,
) -> Result<(Outcome, Duration)> {
    let (program, args) = cmd.split_first().context("no command given")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(old)
        .arg(new)
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

/// Reads `eq` or `neq` from the first non-empty line, ignoring case.
pub fn parse_prediction(output: &str) -> Outcome {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty());
    match line.map(str::to_lowercase).as_deref() {
        Some("eq") => Outcome::Predicted(Kind::Eq),
        Some("neq") => Outcome::Predicted(Kind::Neq),
        _ => Outcome::Invalid(line.unwrap_or_default().chars().take(40).collect()),
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub total: usize,
    pub correct: usize,
    pub total_time: Duration,
    pub slowest: Option<(CaseId, Duration)>,
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
            summary.total_time += r.elapsed;
            if summary.slowest.as_ref().is_none_or(|(_, t)| r.elapsed > *t) {
                summary.slowest = Some((r.id.clone(), r.elapsed));
            }
            match r.outcome {
                Outcome::Predicted(_) => {}
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, timeout: Duration) -> Outcome {
        let cmd = ["sh".to_string(), "-c".to_string(), script.to_string()];
        run_tool(&cmd, Path::new("old"), Path::new("new"), timeout)
            .unwrap()
            .0
    }

    #[test]
    fn parses_predictions() {
        assert_eq!(parse_prediction("eq\n"), Outcome::Predicted(Kind::Eq));
        assert_eq!(parse_prediction("\n  NEQ \nmore"), Outcome::Predicted(Kind::Neq));
        assert_eq!(parse_prediction("maybe"), Outcome::Invalid("maybe".into()));
        assert_eq!(parse_prediction(""), Outcome::Invalid(String::new()));
    }

    #[test]
    fn runs_the_tool_with_both_directories() {
        let second = Duration::from_secs(5);
        // With `sh -c`, the first extra argument is `$0`.
        assert_eq!(
            sh(r#"[ "$0 $1" = "old new" ] && echo neq"#, second),
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

    #[test]
    fn fails_when_the_tool_does_not_exist() {
        let cmd = ["./does-not-exist".to_string()];
        assert!(run_tool(&cmd, Path::new("a"), Path::new("b"), TIMEOUT).is_err());
    }
}
