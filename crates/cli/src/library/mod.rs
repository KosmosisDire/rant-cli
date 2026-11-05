//! Libraries packages build on, such as Rant: which ones the sources name, and where each
//! package takes them from.

mod uses;

pub use uses::{Use, in_folder, node_source, under};

use std::path::Path;

use rant_config::package;
use serde::Serialize;

use crate::packages;

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
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::CMake => "cmake",
            Kind::Python => "python",
            Kind::CSharp => "csharp",
            Kind::Rust => "rust",
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
        }
    }
}

/// The libraries the sources named when last read, so showing what packages use needs no network.
pub fn known(home: &Path) -> Vec<Library> {
    let named = packages::cached(home).into_iter().filter(|s| !s.repo.is_empty());
    named.filter_map(|s| Some(Library { names: s.library?, name: s.name, repo: s.repo })).collect()
}
