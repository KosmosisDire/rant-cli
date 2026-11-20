//! Libraries packages build on, such as Rant: which ones the sources name, and where each
//! package takes them from.

pub mod arduino;
mod edit;
mod machine;
mod moves;
pub mod plcnext;
mod uses;

pub use machine::{carried, copies, install, install_local, uninstall};
pub use moves::{add, all, find, nuget_feed_added, of, update};
pub use uses::{Use, in_folder, node_source, under};

use std::path::Path;

use rant_config::package;
use regex::Regex;
use serde::Serialize;

use crate::packages;

/// A case insensitive pattern, for the build files matched by pattern.
fn icase(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("the patterns here are valid")
}

/// A file's text, empty when it cannot be read.
fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// What `Use::how` says of a machine wide copy placed from a local build, which update leaves alone.
pub const LOCAL_BUILD: &str = "local build";

/// The file of a local build that a release would carry as a file ending in suffix: path itself
/// when it is one, else the newest such file in the folder, its bin or its dist.
fn built(path: &Path, suffix: &str) -> Option<std::path::PathBuf> {
    let named = |p: &Path| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(suffix));
    if path.is_file() {
        return named(path).then(|| path.to_path_buf());
    }
    let found = ["", "bin", "dist"].iter().flat_map(|d| std::fs::read_dir(path.join(d)).into_iter().flatten().flatten()).map(|e| e.path()).filter(|p| p.is_file() && named(p));
    found.max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// The library added when none is named, the one Rant nodes are built on.
pub const DEFAULT_LIBRARY: &str = "rant";

/// A library rant add can add: its name, the GitHub repo whose releases carry it, and the names
/// each language takes it by, as its package.rant gives them.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    pub name: String,
    pub repo: String,
    pub names: package::Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    CMake,
    Python,
    CSharp,
    Rust,
    PlatformIO,
    Arduino,
    /// A plcx workspace, its library in libs/.
    Plcx,
    /// Engineer's user libraries.
    Engineer,
}

/// The kinds that keep a library once per machine for every project, which rant install fills.
pub const MACHINE: [Kind; 2] = [Kind::Arduino, Kind::Engineer];

impl Kind {
    pub fn machine_wide(self) -> bool {
        MACHINE.contains(&self)
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::CMake => "cmake",
            Kind::Python => "python",
            Kind::CSharp => "csharp",
            Kind::Rust => "rust",
            Kind::PlatformIO => "platformio",
            Kind::Arduino => "arduino",
            Kind::Plcx => "plcx",
            Kind::Engineer => "engineer",
        }
    }
}

impl Library {
    /// Whether the library names a way for that kind of package to take it.
    pub fn supports(&self, k: Kind) -> bool {
        let n = &self.names;
        match k {
            Kind::CMake => n.cmake.is_some(),
            Kind::Python => n.python.is_some(),
            Kind::CSharp => n.csharp.is_some(),
            Kind::Rust => n.rust.is_some(),
            Kind::Plcx | Kind::Engineer => n.plcnext.is_some(),
            // the release tells, by the zip it carries
            Kind::PlatformIO | Kind::Arduino => true,
        }
    }
}

/// The libraries the sources named when last read, so showing what packages use needs the
/// network only when the databases were never read.
pub fn known(home: &Path) -> Vec<Library> {
    let named = packages::cached(home, true).into_iter().filter(|s| !s.repo.is_empty());
    named.filter_map(|s| Some(Library { names: s.library?, name: s.name, repo: s.repo })).collect()
}
