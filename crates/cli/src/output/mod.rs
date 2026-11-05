//! Everything rant prints goes through here: data on stdout in the format asked for, notes,
//! warnings and errors on stderr. Color only on a terminal, and never with NO_COLOR set.

mod csv;
mod yaml;

use std::io::{IsTerminal, Write};
use std::path::Path;

use rant_config::paths;
use serde::Serialize;
use serde_json::Value;

/// The explorer's dark palette, so the CLI and the explorer color a thing the same way.
/// Plain keeps the terminal's own foreground, which reads on light and dark backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Dim,
    Faint,
    GreenHi,
    Amber,
    Red,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Style::Plain => "",
            Style::Dim => "38;2;154;154;154",
            Style::Faint => "38;2;100;100;100",
            Style::GreenHi => "38;2;92;166;125",
            Style::Amber => "38;2;207;154;58",
            Style::Red => "38;2;210;87;75",
        }
    }
}

fn paint(color: bool, s: Style, text: &str) -> String {
    if !color || s == Style::Plain { text.to_string() } else { format!("\x1b[{}m{text}\x1b[0m", s.code()) }
}

/// How data is written: as text for a person, or as JSON or CSV for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Csv,
}

/// What a command prints: a value a program reads as JSON or CSV, and a person as text.
pub trait Data: Serialize {
    /// The text a person reads, YAML of the data unless a command shows it its own way.
    fn text(&self, t: &Text) -> String {
        t.yaml(&serde_json::to_value(self).unwrap_or(Value::Null))
    }
}

/// What a command writes its text with: the color it may use, and paths seen from where rant runs.
pub struct Text<'a> {
    color: bool,
    from: Option<&'a Path>,
}

impl Text<'_> {
    /// Relative to where rant runs, or whole when that is shorter.
    pub fn path(&self, p: &Path) -> String {
        shown(p, self.from)
    }

    pub fn yaml(&self, v: &Value) -> String {
        yaml::block(v, self.color)
    }
}

pub struct Output {
    format: Format,
    color_out: bool,
    color_err: bool,
}

impl Output {
    pub fn new(format: Format) -> Output {
        Output { format, color_out: takes_color(Stream::Out), color_err: takes_color(Stream::Err) }
    }

    /// A command's data in the format asked for. `from` is where paths in text count from.
    pub fn show<D: Data>(&self, data: &D, from: Option<&Path>) {
        let text = match self.format {
            Format::Json => serde_json::to_string_pretty(data).unwrap_or_default(),
            Format::Csv => csv::table(&serde_json::to_value(data).unwrap_or(Value::Null)),
            Format::Text => data.text(&Text { color: self.color_out, from }),
        };
        if !text.is_empty() {
            let _ = writeln!(std::io::stdout(), "{text}");
        }
    }

    pub fn error(&self, text: &str) {
        self.say(&format!("{} {text}", paint(self.color_err, Style::Red, "error:")));
    }

    pub fn warn(&self, text: &str) {
        self.say(&format!("{} {text}", paint(self.color_err, Style::Amber, "warning:")));
    }

    /// A line on stderr. A pipe closed under rant is no reason to fail, so errors are dropped.
    fn say(&self, line: &str) {
        let _ = std::io::stdout().flush();
        let _ = writeln!(std::io::stderr(), "{line}");
    }
}

/// A path relative to `from`, or whole when that is shorter or on another drive.
pub fn shown(p: &Path, from: Option<&Path>) -> String {
    let whole = paths::slash(p);
    match from.and_then(|f| relative(p, f)) {
        Some(near) if near.len() <= whole.len() => near,
        _ => whole,
    }
}

/// p as seen from base, climbing with `..` where it must. None when they share no root.
fn relative(p: &Path, base: &Path) -> Option<String> {
    let (p, base) = (paths::normalize(p), paths::normalize(base));
    let a: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let b: Vec<String> = base.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let same = |x: &String, y: &String| if cfg!(windows) { x.to_lowercase() == y.to_lowercase() } else { x == y };
    let common = a.iter().zip(&b).take_while(|(x, y)| same(x, y)).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<&str> = vec![".."; b.len() - common];
    parts.extend(a[common..].iter().map(String::as_str));
    Some(if parts.is_empty() { ".".to_string() } else { parts.join("/") })
}

enum Stream {
    Out,
    Err,
}

fn takes_color(stream: Stream) -> bool {
    let terminal = match stream {
        Stream::Out => std::io::stdout().is_terminal(),
        Stream::Err => std::io::stderr().is_terminal(),
    };
    terminal && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && escapes(stream)
}

/// Turns on ANSI escapes for a Windows console. A mintty pipe has no console mode and shows
/// escapes as they are.
#[cfg(windows)]
fn escapes(stream: Stream) -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
    };
    let which = match stream {
        Stream::Out => STD_OUTPUT_HANDLE,
        Stream::Err => STD_ERROR_HANDLE,
    };
    // SAFETY: the standard handle is owned by the process and only its mode changes.
    unsafe {
        let h = GetStdHandle(which);
        let mut mode = 0;
        GetConsoleMode(h, &mut mode) == 0 || SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn escapes(_: Stream) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_shown_from_where_rant_runs_when_that_is_shorter() {
        let base = std::env::temp_dir().join("ws");
        assert_eq!(shown(&base.join("a/b.txt"), Some(&base)), "a/b.txt");
        assert_eq!(shown(&base, Some(&base)), ".");
        assert_eq!(shown(&base.join("CMakeLists.txt"), Some(&base.join("nodes"))), "../CMakeLists.txt");
        let far = base.join("x");
        let deep = base.join("a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z");
        assert_eq!(shown(&far, Some(&deep)), paths::slash(&far), "a long climb reads worse than the whole path");
        assert_eq!(shown(&far, None), paths::slash(&far));
    }

    #[test]
    fn paint_only_with_color() {
        assert_eq!(paint(false, Style::Red, "x"), "x");
        assert_eq!(paint(true, Style::Plain, "x"), "x");
        assert_eq!(paint(true, Style::Red, "x"), "\x1b[38;2;210;87;75mx\x1b[0m");
    }
}
