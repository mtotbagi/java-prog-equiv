use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};

/// Name of the original version of a program in a case directory.
pub const OLD_FILE: &str = "oldV.java";
/// Name of the modified version of a program in a case directory.
pub const NEW_FILE: &str = "newV.java";

/// Ground truth of a case, given by the name of its directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Eq,
    Neq,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Eq => "Eq",
            Kind::Neq => "Neq",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "Eq" => Ok(Kind::Eq),
            "Neq" => Ok(Kind::Neq),
            _ => bail!("expected `Eq` or `Neq`, got `{s}`"),
        }
    }
}

/// Identifier of a case: `<bench>-<prog>-<Eq|Neq>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CaseId {
    pub bench: String,
    pub prog: String,
    pub kind: Kind,
}

impl fmt::Display for CaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.bench, self.prog, self.kind)
    }
}

impl FromStr for CaseId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        let [bench, prog, kind] = parts[..] else {
            bail!("invalid case id `{s}`, expected `<bench>-<prog>-<Eq|Neq>`");
        };
        if bench.is_empty() || prog.is_empty() {
            bail!("invalid case id `{s}`, expected `<bench>-<prog>-<Eq|Neq>`");
        }
        Ok(CaseId {
            bench: bench.to_string(),
            prog: prog.to_string(),
            kind: kind.parse().with_context(|| format!("invalid case id `{s}`"))?,
        })
    }
}

/// A pair of Java programs together with its ground truth.
#[derive(Debug, Clone)]
pub struct Case {
    pub id: CaseId,
    /// Directory of the case, e.g. `benchmarks/caldat/caldat/Neq`.
    pub dir: PathBuf,
    pub old: PathBuf,
    pub new: PathBuf,
}

/// All the Java cases of the dataset.
#[derive(Debug, Clone)]
pub struct Dataset {
    pub root: PathBuf,
    pub cases: Vec<Case>,
}

impl Dataset {
    /// Discovers every case under `root` (the `benchmarks` directory).
    ///
    /// The layout is `<root>/<bench>/<prog>/<Eq|Neq>/{oldV,newV}.java`.
    /// Only the Java files are considered, the C versions are ignored.
    pub fn load(root: &Path) -> Result<Self> {
        let mut cases = Vec::new();
        for bench_dir in subdirs(root)? {
            for prog_dir in subdirs(&bench_dir)? {
                for kind_dir in subdirs(&prog_dir)? {
                    if let Some(case) = load_case(&kind_dir)? {
                        cases.push(case);
                    }
                }
            }
        }
        cases.sort_by_cached_key(|c| {
            (
                c.id.bench.to_lowercase(),
                c.id.prog.to_lowercase(),
                c.id.kind,
            )
        });
        Ok(Dataset {
            root: root.to_path_buf(),
            cases,
        })
    }

    pub fn find(&self, id: &CaseId) -> Option<&Case> {
        self.cases.iter().find(|c| &c.id == id)
    }
}

fn load_case(dir: &Path) -> Result<Option<Case>> {
    let Ok(kind) = file_name(dir).parse::<Kind>() else {
        return Ok(None);
    };
    let old = dir.join(OLD_FILE);
    let new = dir.join(NEW_FILE);
    match (old.is_file(), new.is_file()) {
        (false, false) => return Ok(None),
        (true, true) => {}
        _ => bail!("{} should contain both {OLD_FILE} and {NEW_FILE}", dir.display()),
    }
    let prog_dir = dir.parent().context("case directory has no parent")?;
    let bench_dir = prog_dir.parent().context("program directory has no parent")?;
    Ok(Some(Case {
        id: CaseId {
            bench: file_name(bench_dir).to_string(),
            prog: file_name(prog_dir).to_string(),
            kind,
        },
        dir: dir.to_path_buf(),
        old,
        new,
    }))
}

fn subdirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_id_roundtrip() {
        let id: CaseId = "caldat-caldat-Neq".parse().unwrap();
        assert_eq!(id.bench, "caldat");
        assert_eq!(id.prog, "caldat");
        assert_eq!(id.kind, Kind::Neq);
        assert_eq!(id.to_string(), "caldat-caldat-Neq");
    }

    #[test]
    fn case_id_rejects_malformed() {
        for bad in ["", "caldat", "caldat-Neq", "a-b-c-Eq", "a-b-eq", "-b-Eq", "a--Eq"] {
            assert!(bad.parse::<CaseId>().is_err(), "`{bad}` should be rejected");
        }
    }
}
