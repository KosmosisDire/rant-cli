//! Programs run from an argv, never through a shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where a program runs from. A name holding a slash counts from cwd, a bare name is looked
/// up on PATH, through PATHEXT on Windows, and never in the current folder.
pub fn find_program(program: &str, cwd: &Path) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let p = Path::new(program);
    if has_separator(program) || p.is_absolute() {
        return runnable(&cwd.join(p));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).filter(|d| !d.as_os_str().is_empty()).find_map(|d| runnable(&d.join(p)))
}

/// What a command prints on stdout, None when it cannot start or exits nonzero. Its stderr
/// is dropped, since a caller only probes.
pub fn output(argv: &[String], cwd: &Path) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let exe = find_program(program, cwd)?;
    let done = Command::new(exe).args(args).current_dir(cwd).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    done.status.success().then(|| String::from_utf8_lossy(&done.stdout).into_owned())
}

fn has_separator(s: &str) -> bool {
    s.contains('/') || (cfg!(windows) && s.contains('\\'))
}

/// The file the OS runs for p: on Windows p with an executable extension, or p with each
/// PATHEXT extension in turn.
#[cfg(windows)]
fn runnable(p: &Path) -> Option<PathBuf> {
    let exts = std::env::var("PATHEXT").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    let exts: Vec<String> = exts.split(';').filter(|e| !e.is_empty()).map(str::to_lowercase).collect();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()));
    if ext.is_some_and(|e| exts.contains(&e)) && p.is_file() {
        return Some(p.to_path_buf());
    }
    exts.iter()
        .map(|e| {
            let mut name = p.as_os_str().to_owned();
            name.push(e);
            PathBuf::from(name)
        })
        .find(|c| c.is_file())
}

#[cfg(unix)]
fn runnable(p: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    executable.then(|| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_is_found_on_path_and_a_path_counts_from_cwd() {
        let here = std::env::current_dir().unwrap();
        assert!(find_program("cargo", &here).is_some(), "cargo runs these tests, so it is on PATH");
        assert!(find_program("./missing", &here).is_none());
        assert!(find_program("", &here).is_none());
        let cargo = find_program("cargo", &here).unwrap();
        assert_eq!(find_program(cargo.to_str().unwrap(), Path::new("/elsewhere")), Some(cargo.clone()));
    }

    #[test]
    fn output_is_what_a_program_prints() {
        let here = std::env::current_dir().unwrap();
        let version = output(&["cargo".into(), "--version".into()], &here).unwrap();
        assert!(version.starts_with("cargo "), "{version}");
        assert_eq!(output(&["cargo".into(), "no-such-command".into()], &here), None, "a failure is no output");
    }
}
