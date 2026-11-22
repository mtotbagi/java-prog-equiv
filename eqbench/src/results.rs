//! Results of `eqbench analyse` as JSON lines: one record per case, written
//! as soon as the case is analysed, so that long runs can be plotted or
//! resumed.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::analyse::{CaseResult, Outcome};
use crate::case::Kind;

/// Settings of a run, shared by all its records.
#[derive(Debug, Clone)]
pub struct Run {
    /// The analysis command, as given to `analyse`.
    pub tool: String,
    pub timeout: Duration,
    /// Memory limit, in MiB.
    pub memory: Option<u64>,
    pub jobs: usize,
}

/// One line of a results file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Case id, `<bench>-<prog>-<Eq|Neq>`.
    pub id: String,
    /// `eq` or `neq`.
    pub expected: String,
    /// `eq`, `neq`, `unknown`, `invalid`, `crash`, `timeout`, `memory out` or
    /// `not built`.
    pub outcome: String,
    /// First line of an invalid output, exit code of a crash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Wall time of the tool on the case.
    pub seconds: f64,
    /// Peak resident memory of the tool, in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mib: Option<f64>,
    pub tool: String,
    /// Timeout of the run, in seconds.
    pub timeout: u64,
    /// Memory limit of the run, in MiB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit_mib: Option<u64>,
    /// Number of cases analysed at the same time.
    pub jobs: usize,
}

impl Record {
    pub fn new(result: &CaseResult, run: &Run) -> Self {
        let (outcome, detail) = match &result.outcome {
            Outcome::Predicted(Kind::Eq) => ("eq", None),
            Outcome::Predicted(Kind::Neq) => ("neq", None),
            Outcome::Unknown => ("unknown", None),
            Outcome::Invalid(output) => ("invalid", Some(output.clone())),
            Outcome::Crashed(code) => ("crash", code.map(|c| c.to_string())),
            Outcome::Timeout => ("timeout", None),
            Outcome::MemoryOut => ("memory out", None),
            Outcome::NotBuilt => ("not built", None),
        };
        Record {
            id: result.id.to_string(),
            expected: Outcome::Predicted(result.id.kind).label(),
            outcome: outcome.to_string(),
            detail,
            seconds: result.elapsed.as_secs_f64(),
            memory_mib: result.memory.map(|kib| kib as f64 / 1024.0),
            tool: run.tool.clone(),
            timeout: run.timeout.as_secs(),
            memory_limit_mib: run.memory,
            jobs: run.jobs,
        }
    }

    /// The result the record was made from.
    pub fn to_result(&self) -> Result<CaseResult> {
        let outcome = match self.outcome.as_str() {
            "eq" => Outcome::Predicted(Kind::Eq),
            "neq" => Outcome::Predicted(Kind::Neq),
            "unknown" => Outcome::Unknown,
            "invalid" => Outcome::Invalid(self.detail.clone().unwrap_or_default()),
            "crash" => Outcome::Crashed(self.detail.as_deref().map(str::parse).transpose()?),
            "timeout" => Outcome::Timeout,
            "memory out" => Outcome::MemoryOut,
            "not built" => Outcome::NotBuilt,
            other => bail!("unknown outcome `{other}` for {}", self.id),
        };
        Ok(CaseResult {
            id: self.id.parse()?,
            outcome,
            elapsed: Duration::from_secs_f64(self.seconds),
            memory: self.memory_mib.map(|mib| (mib * 1024.0).round() as u64),
        })
    }
}

/// Reads a results file. A last line that does not parse is dropped: it was
/// being written when the run was killed.
pub fn load(path: &Path) -> Result<Vec<Record>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut records = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str(line) {
            Ok(record) => records.push(record),
            Err(_) if i + 1 == lines.len() => {
                eprintln!("{}: dropping the truncated last line", path.display())
            }
            Err(e) => bail!("{}:{}: {e}", path.display(), i + 1),
        }
    }
    Ok(records)
}

/// A results file being written, line by line.
pub struct Writer {
    file: File,
}

impl Writer {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let file =
            File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
        Ok(Writer { file })
    }

    /// Appends `record`, written at once so that a killed run loses nothing
    /// but the case it was on.
    pub fn write(&mut self, record: &Record) -> Result<()> {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .context("cannot write the results")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> Run {
        Run {
            tool: "./my-tool --fast".to_string(),
            timeout: Duration::from_secs(10),
            memory: Some(2048),
            jobs: 6,
        }
    }

    fn record(outcome: Outcome) -> Record {
        let result = CaseResult {
            id: "caldat-caldat-Neq".parse().unwrap(),
            outcome,
            elapsed: Duration::from_millis(1500),
            memory: Some(512 * 1024),
        };
        Record::new(&result, &run())
    }

    #[test]
    fn records_a_result() {
        let r = record(Outcome::Predicted(Kind::Eq));
        assert_eq!(r.id, "caldat-caldat-Neq");
        assert_eq!(r.expected, "neq");
        assert_eq!(r.outcome, "eq");
        assert_eq!(r.detail, None);
        assert_eq!(r.seconds, 1.5);
        assert_eq!(
            (r.tool.as_str(), r.timeout, r.jobs),
            ("./my-tool --fast", 10, 6)
        );
        assert_eq!(
            record(Outcome::Crashed(Some(3))).detail.as_deref(),
            Some("3")
        );
        assert_eq!(record(Outcome::Invalid("maybe".into())).outcome, "invalid");
    }

    #[test]
    fn reads_back_every_outcome() {
        for outcome in [
            Outcome::Predicted(Kind::Eq),
            Outcome::Predicted(Kind::Neq),
            Outcome::Unknown,
            Outcome::Invalid("maybe".into()),
            Outcome::Crashed(Some(3)),
            Outcome::Crashed(None),
            Outcome::Timeout,
            Outcome::MemoryOut,
            Outcome::NotBuilt,
        ] {
            let result = record(outcome.clone()).to_result().unwrap();
            assert_eq!(result.outcome, outcome);
            assert_eq!(result.elapsed, Duration::from_millis(1500));
            assert_eq!(result.memory, Some(512 * 1024));
        }
    }

    #[test]
    fn drops_a_truncated_last_line() {
        let path =
            std::env::temp_dir().join(format!("eqbench-results-{}.jsonl", std::process::id()));
        let mut writer = Writer::create(&path).unwrap();
        writer.write(&record(Outcome::Unknown)).unwrap();
        writer.write(&record(Outcome::Timeout)).unwrap();
        writer.file.write_all(b"{\"id\":\"caldat-cal").unwrap();
        let records = load(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[1].outcome, "timeout");
    }

    #[test]
    fn writes_one_json_object_per_line() {
        let r = record(Outcome::Timeout);
        let line = serde_json::to_string(&r).unwrap();
        assert!(!line.contains('\n'));
        assert!(!line.contains("detail"));
        assert_eq!(serde_json::from_str::<Record>(&line).unwrap(), r);
    }
}
