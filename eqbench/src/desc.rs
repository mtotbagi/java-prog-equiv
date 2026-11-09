//! Parsing of the `J-Desc.json` files describing a Java case.
//!
//! The files follow `EqDescTemplate.json` / `NeqDescTemplate.json` but are
//! hand written, so the values are normalised here:
//! - `change line` is either a number (`21`) or a string (`"4-13"`, `"7,9-10,16"`),
//! - `change type` comes in several spellings (`Insert`, `Insertion`, `update`, ...),
//! - `counter-example` keys are usually `"<type> <name>"` but sometimes only a
//!   field path such as `"this.x"`, and values may be Java expressions.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Name of the description file of the Java versions of a case.
pub const DESC_FILE: &str = "J-Desc.json";

#[derive(Debug, Clone, PartialEq)]
pub struct Desc {
    pub benchmark: String,
    pub program: String,
    pub method: String,
    pub loc: u32,
    pub loops: u32,
    pub non_linear: u32,
    pub changes: Vec<Change>,
    /// Inputs on which the two versions differ (non-equivalent cases only).
    pub counter_example: Option<Vec<Param>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub lines: Vec<LineRange>,
    pub kind: ChangeKind,
    pub operation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}

/// An inclusive range of source lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

/// One input of a counter-example.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Java type of the input, absent for keys like `this.x`.
    pub ty: Option<String>,
    pub name: String,
    pub value: Value,
}

#[derive(Deserialize)]
struct RawDesc {
    #[serde(rename = "benchmark name")]
    benchmark: String,
    #[serde(rename = "program name")]
    program: String,
    #[serde(rename = "method name")]
    method: String,
    #[serde(rename = "LOC")]
    loc: u32,
    #[serde(rename = "# Loops")]
    loops: u32,
    #[serde(rename = "# non-linear arithmetics")]
    non_linear: u32,
    changes: Vec<RawChange>,
    #[serde(rename = "counter-example")]
    counter_example: Option<Vec<Map<String, Value>>>,
}

#[derive(Deserialize)]
struct RawChange {
    #[serde(rename = "change line")]
    line: Value,
    #[serde(rename = "change type")]
    kind: String,
    #[serde(rename = "change operation")]
    operation: String,
}

impl Desc {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("invalid description {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawDesc = serde_json::from_str(text)?;
        let changes = raw
            .changes
            .into_iter()
            .map(|c| {
                Ok(Change {
                    lines: parse_line_value(&c.line)?,
                    kind: c.kind.parse()?,
                    operation: c.operation.trim().to_string(),
                })
            })
            .collect::<Result<_>>()?;
        let counter_example = raw.counter_example.map(|entries| {
            entries
                .into_iter()
                .flatten()
                .map(|(key, value)| Param::new(&key, value))
                .collect()
        });
        Ok(Desc {
            benchmark: raw.benchmark,
            program: raw.program,
            method: raw.method,
            loc: raw.loc,
            loops: raw.loops,
            non_linear: raw.non_linear,
            changes,
            counter_example,
        })
    }
}

impl std::str::FromStr for ChangeKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "insert" | "insertion" => Ok(ChangeKind::Insert),
            "update" => Ok(ChangeKind::Update),
            "delete" | "deletion" => Ok(ChangeKind::Delete),
            _ => bail!("unknown change type `{s}`"),
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChangeKind::Insert => "Insert",
            ChangeKind::Update => "Update",
            ChangeKind::Delete => "Delete",
        })
    }
}

impl fmt::Display for LineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            write!(f, "{}", self.start)
        } else {
            write!(f, "{}-{}", self.start, self.end)
        }
    }
}

impl Param {
    fn new(key: &str, value: Value) -> Self {
        match key.trim().rsplit_once(char::is_whitespace) {
            Some((ty, name)) => Param {
                ty: Some(ty.trim().to_string()),
                name: name.to_string(),
                value,
            },
            None => Param {
                ty: None,
                name: key.trim().to_string(),
                value,
            },
        }
    }
}

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ty) = &self.ty {
            write!(f, "{ty} ")?;
        }
        match &self.value {
            Value::String(s) => write!(f, "{} = {s}", self.name),
            v => write!(f, "{} = {v}", self.name),
        }
    }
}

fn parse_line_value(value: &Value) -> Result<Vec<LineRange>> {
    match value {
        Value::Number(n) => {
            let line = n
                .as_u64()
                .and_then(|l| u32::try_from(l).ok())
                .with_context(|| format!("invalid change line `{n}`"))?;
            Ok(vec![LineRange {
                start: line,
                end: line,
            }])
        }
        Value::String(s) => parse_lines(s),
        v => bail!("invalid change line `{v}`"),
    }
}

/// Parses a list of lines such as `"4,6-8"` into ranges.
pub fn parse_lines(s: &str) -> Result<Vec<LineRange>> {
    s.split(',')
        .map(|part| {
            let part = part.trim();
            let (start, end) = part.split_once('-').unwrap_or((part, part));
            let start: u32 = start
                .trim()
                .parse()
                .with_context(|| format!("invalid change line `{s}`"))?;
            let end: u32 = end
                .trim()
                .parse()
                .with_context(|| format!("invalid change line `{s}`"))?;
            if start > end {
                bail!("invalid change line `{s}`: {start} > {end}");
            }
            Ok(LineRange { start, end })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: u32, end: u32) -> LineRange {
        LineRange { start, end }
    }

    #[test]
    fn parses_line_lists() {
        assert_eq!(parse_lines("12").unwrap(), vec![range(12, 12)]);
        assert_eq!(parse_lines("4-13").unwrap(), vec![range(4, 13)]);
        assert_eq!(
            parse_lines("7,9-10, 16").unwrap(),
            vec![range(7, 7), range(9, 10), range(16, 16)]
        );
        assert!(parse_lines("10-4").is_err());
        assert!(parse_lines("").is_err());
        assert!(parse_lines("a-b").is_err());
    }

    #[test]
    fn normalises_change_kinds() {
        assert_eq!("Insertion".parse::<ChangeKind>().unwrap(), ChangeKind::Insert);
        assert_eq!("update".parse::<ChangeKind>().unwrap(), ChangeKind::Update);
        assert_eq!("Delete".parse::<ChangeKind>().unwrap(), ChangeKind::Delete);
        assert!("Rename".parse::<ChangeKind>().is_err());
    }

    #[test]
    fn splits_counter_example_keys() {
        let p = Param::new("double x1", Value::from(0.5));
        assert_eq!(p.ty.as_deref(), Some("double"));
        assert_eq!(p.name, "x1");
        assert_eq!(p.to_string(), "double x1 = 0.5");

        let p = Param::new("this.center.x", Value::from(1));
        assert_eq!(p.ty, None);
        assert_eq!(p.to_string(), "this.center.x = 1");

        let p = Param::new("Vector3D v", Value::from("new Vector3D(1.0, 2.0, 3.0)"));
        assert_eq!(p.to_string(), "Vector3D v = new Vector3D(1.0, 2.0, 3.0)");
    }

    #[test]
    fn parses_a_description() {
        let desc = Desc::parse(
            r##"{
              "benchmark name": "caldat",
              "program name": "caldat",
              "method name": "caldat",
              "LOC": 23,
              "# Loops": 0,
              "# non-linear arithmetics": 9,
              "changes": [
                { "change line": 21, "change type": "Delete", "change operation": "Condition" },
                { "change line": "4,6-8", "change type": "Insertion", "change operation": "Dummy statement" }
              ],
              "counter-example": [ {"int julian": 100} ]
            }"##,
        )
        .unwrap();
        assert_eq!(desc.method, "caldat");
        assert_eq!(desc.loc, 23);
        assert_eq!(desc.changes.len(), 2);
        assert_eq!(desc.changes[1].kind, ChangeKind::Insert);
        assert_eq!(desc.changes[1].lines, vec![range(4, 4), range(6, 8)]);
        let ce = desc.counter_example.unwrap();
        assert_eq!(ce[0].to_string(), "int julian = 100");
    }
}
