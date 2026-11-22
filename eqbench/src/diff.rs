//! Git-like diff between the two versions of a case.
//!
//! The programs are small, so the whole file is printed with the usual
//! `--- a/` / `+++ b/` header and ` `, `-`, `+` markers, without hunks.

use similar::{ChangeTag, TextDiff};

use crate::term::Style;

pub fn whole_file_diff(
    old_label: &str,
    old: &str,
    new_label: &str,
    new: &str,
    style: Style,
) -> String {
    let mut out = String::new();
    out.push_str(&style.bold(&format!("--- a/{old_label}")));
    out.push('\n');
    out.push_str(&style.bold(&format!("+++ b/{new_label}")));
    out.push('\n');

    let diff = TextDiff::from_lines(old, new);
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches(['\n', '\r']);
        let line = match change.tag() {
            ChangeTag::Equal => format!(" {line}"),
            ChangeTag::Delete => style.red(&format!("-{line}")),
            ChangeTag::Insert => style.green(&format!("+{line}")),
        };
        out.push_str(&line);
        out.push('\n');
        if change.missing_newline() {
            out.push_str("\\ No newline at end of file\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_the_whole_file_without_hunks() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let diff = whole_file_diff("x/oldV.java", old, "x/newV.java", new, Style::plain());
        assert_eq!(
            diff,
            "--- a/x/oldV.java\n+++ b/x/newV.java\n a\n-b\n+B\n c\n"
        );
    }

    #[test]
    fn handles_crlf_and_missing_final_newline() {
        let diff = whole_file_diff("o", "a\r\nb", "n", "a\r\nc", Style::plain());
        assert_eq!(
            diff,
            "--- a/o\n+++ b/n\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
        );
    }
}
