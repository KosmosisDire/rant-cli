#![allow(dead_code)]

#[path = "../../src/testdir.rs"]
mod testdir;

use std::path::Path;

use rant_config::{Settings, Workspace};
pub use testdir::TestDir;

pub fn load(t: &TestDir) -> Workspace {
    load_at(t, "")
}

/// The workspace around rel, seen from there, with no package installed.
pub fn load_at(t: &TestDir, rel: &str) -> Workspace {
    load_with_home(t, rel, &t.path(".home"))
}

pub fn load_with_home(t: &TestDir, rel: &str, home: &Path) -> Workspace {
    let here = t.path(rel);
    let (ws, diags) = Workspace::load(Settings::find(&here, home).unwrap(), &here, home);
    assert!(diags.is_empty(), "{diags:?}");
    ws
}
