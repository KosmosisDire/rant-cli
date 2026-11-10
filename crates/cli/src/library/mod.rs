//! Libraries packages build on, such as Rant: which ones the sources name, and where each
//! package takes them from.

pub mod arduino;
mod edit;
mod moves;
pub mod plcnext;
mod uses;

pub use moves::{add, all, find, nuget_feed_added, update};
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
    /// An Engineer project left as its archive, its library in Engineer's user libraries.
    Engineer,
}

impl Kind {
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
