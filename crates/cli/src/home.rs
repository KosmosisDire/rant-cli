//! rant's home, ~/.rant: installed packages, logs and state outside every workspace, the
//! explorer, mesh snapshots and shell hooks. RANT_HOME moves it, which the tests use.

use std::path::PathBuf;

pub fn rant_home() -> Option<PathBuf> {
    env_path("RANT_HOME").or_else(|| home_dir().map(|h| h.join(".rant")))
}

/// HOME, else USERPROFILE on Windows.
pub fn home_dir() -> Option<PathBuf> {
    env_path("HOME").or_else(|| if cfg!(windows) { env_path("USERPROFILE") } else { None })
}

/// This rant's own program file.
pub fn program() -> crate::failure::Result<PathBuf> {
    std::env::current_exe().map_err(|e| crate::failure::failed(format!("cannot find rant itself: {e}")))
}

/// A variable naming a path, None when unset or empty.
pub fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}
