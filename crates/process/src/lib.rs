//! The programs rant-cli runs from an argv, never through a shell: tools in the foreground,
//! and nodes started detached, found again in a later run and stopped with all they started.

pub mod quote;

#[cfg(unix)]
mod posix;
#[cfg(unix)]
use posix as os;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use os::{Child, alive, helper, owns, pid_alive, start_child, stop, stop_pid};

#[derive(Debug, Clone, Default)]
pub struct Command {
    /// argv[0] is the program, looked up as find_program does.
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    /// Set on top of the inherited environment.
    pub env: BTreeMap<String, String>,
    /// `NAME=value` entries inherited in place of this process's own.
    pub base: Option<Vec<String>>,
}

impl Command {
    pub fn new<S: Into<String>>(argv: impl IntoIterator<Item = S>, cwd: impl Into<PathBuf>) -> Command {
        Command { argv: argv.into_iter().map(Into::into).collect(), cwd: cwd.into(), ..Command::default() }
    }
}

/// Why a program could not be run, found or tracked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn error(message: impl Into<String>) -> Error {
    Error(message.into())
}

/// What finds a started process again in a later run of rant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tracking {
    /// The spawned process, also the process group id on POSIX.
    pub pid: u64,
    /// Its start time, which tells it from a later process given the same id.
    pub start_time: u64,
    /// The Windows job object's name, empty on POSIX.
    pub job: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    Gracefully,
    Killed,
    KilledAtOnce,
    AlreadyGone,
}

/// Starts c detached with stdin from the null device and its output appended to log. job
/// names the Windows job object and is ignored elsewhere.
pub fn start_detached(c: &Command, log: &Path, job: &str) -> Result<Tracking> {
    start_child(c, log, job).map(|child| child.tracking().clone())
}

/// Runs a tool to its end and gives its exit code. Its stdin is this process's, and what it
/// prints goes to this process's stderr, since stdout carries only rant's own data.
pub fn run(c: &Command) -> Result<i32> {
    os::run(c)
}

/// What a command prints on stdout, None when it cannot start or exits nonzero. Its stderr
/// is dropped, since a caller only probes.
pub fn output(c: &Command) -> Option<String> {
    os::output(c)
}

/// Where a program runs from. A name holding a slash counts from cwd, a bare name is looked
/// up on PATH, through PATHEXT on Windows, and never in the current folder.
pub fn find_program(program: &str, cwd: &Path) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let p = Path::new(program);
    if has_separator(program) || p.is_absolute() {
        return os::runnable(&cwd.join(p));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).find_map(|d| os::runnable(&d.join(p)))
}

/// find_program for argv[0], or an error saying where it was looked for.
fn require_program(c: &Command) -> Result<PathBuf> {
    let name = c.argv.first().filter(|n| !n.is_empty()).ok_or_else(|| error("the command is empty"))?;
    if let Some(p) = find_program(name, &c.cwd) {
        return Ok(p);
    }
    let p = Path::new(name);
    if !has_separator(name) && !p.is_absolute() {
        return Err(error(format!("`{name}` was not found on PATH")));
    }
    let place = if p.is_relative() { format!(" in {}", c.cwd.display()) } else { String::new() };
    Err(error(format!("`{name}` was not found{place}")))
}

fn has_separator(s: &str) -> bool {
    s.contains('/') || (cfg!(windows) && s.contains('\\'))
}

/// The inherited environment, or the command's base, with its env on top, as `NAME=value`
/// entries sorted by name. Names match without case on Windows.
pub fn environment(c: &Command) -> Vec<String> {
    let key = |name: &str| if cfg!(windows) { name.to_lowercase() } else { name.to_string() };
    // a Windows entry such as `=C:=C:\x` keeps its leading `=` in the name
    let name_of = |entry: &str| entry.char_indices().skip(1).find(|(_, ch)| *ch == '=').map_or(entry, |(i, _)| &entry[..i]).to_string();
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    let inherited = c.base.clone().unwrap_or_else(os::inherited_environment);
    for entry in inherited {
        merged.insert(key(&name_of(&entry)), entry);
    }
    for (k, v) in &c.env {
        merged.insert(key(k), format!("{k}={v}"));
    }
    merged.into_values().collect()
}

/// The words of a command for messages, quoted where a word holds a space.
pub fn shown(argv: &[String]) -> String {
    let word = |a: &String| if a.is_empty() || a.contains([' ', '\t', '"', '\'']) { format!("'{a}'") } else { a.clone() };
    argv.iter().map(word).collect::<Vec<_>>().join(" ")
}

/// Waits until done says so or limit passes. True when it did.
fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while !done() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_takes_overrides_and_a_base_replaces_what_is_inherited() {
        let mut c = Command::default();
        c.env.insert("RANT_UNIT_X".into(), "1".into());
        assert!(environment(&c).contains(&"RANT_UNIT_X=1".to_string()));
        c.base = Some(vec!["RANT_UNIT_A=1".into(), "rant_unit_x=0".into()]);
        let expected: &[&str] = if cfg!(windows) { &["RANT_UNIT_A=1", "RANT_UNIT_X=1"] } else { &["RANT_UNIT_A=1", "RANT_UNIT_X=1", "rant_unit_x=0"] };
        assert_eq!(environment(&c), expected);
    }

    #[test]
    fn a_program_is_found_on_path_or_from_cwd() {
        let here = std::env::current_dir().unwrap();
        let cargo = find_program("cargo", &here).expect("cargo runs these tests, so it is on PATH");
        assert_eq!(find_program(cargo.to_str().unwrap(), Path::new("/elsewhere")), Some(cargo));
        assert!(find_program("./missing", &here).is_none());
        let missing = require_program(&Command::new(["no-such-program-rant-test"], &here)).unwrap_err();
        assert_eq!(missing.0, "`no-such-program-rant-test` was not found on PATH");
    }

    #[test]
    fn a_tool_gives_its_exit_code_and_a_probe_its_output() {
        let here = std::env::current_dir().unwrap();
        assert_eq!(run(&Command::new(["cargo", "--version"], &here)), Ok(0));
        assert_ne!(run(&Command::new(["cargo", "no-such-command"], &here)), Ok(0));
        assert!(run(&Command::new(["no-such-program-rant-test"], &here)).is_err());
        assert!(output(&Command::new(["cargo", "--version"], &here)).unwrap().starts_with("cargo "));
        assert_eq!(output(&Command::new(["cargo", "no-such-command"], &here)), None);
    }

    #[test]
    fn shown_quotes_words_with_spaces() {
        assert_eq!(shown(&["cmake".into(), "-E".into(), "echo".into(), "a b".into(), String::new()]), "cmake -E echo 'a b' ''");
    }
}
