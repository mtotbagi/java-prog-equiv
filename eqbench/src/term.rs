//! Minimal ANSI styling, enabled only when writing to a terminal.

use std::io::IsTerminal;

#[derive(Debug, Clone, Copy)]
pub struct Style {
    color: bool,
}

impl Style {
    /// Colors when stdout is a terminal and `NO_COLOR` is not set.
    pub fn stdout() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Style {
            color: std::io::stdout().is_terminal() && !no_color,
        }
    }

    pub fn plain() -> Self {
        Style { color: false }
    }

    fn paint(self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(self, text: &str) -> String {
        self.paint("1", text)
    }

    pub fn red(self, text: &str) -> String {
        self.paint("31", text)
    }

    pub fn green(self, text: &str) -> String {
        self.paint("32", text)
    }

    pub fn dim(self, text: &str) -> String {
        self.paint("2", text)
    }
}
