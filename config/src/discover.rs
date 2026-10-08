//! The walk that finds packages, group files and Python nodes: every directory under the
//! workspace root that declares `package {}` or has a manifest that uses Rant, every
//! `*.group.hcl` file, and every Python file that is a node, skipping gitignored paths and
//! the workspace `ignore` globs.

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::diag::Diag;
use crate::manifest::{self, Manifest};
use crate::package::{self, PackageBlock};
use crate::paths;
use crate::scan::{skipped_dir, Cache, NodeKind};
use crate::workspace::{WorkspaceConfig, MANIFEST};

/// A directory that is a package, before it is named and scanned.
pub struct Candidate {
    pub dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub block: Option<PackageBlock>,
}

/// What one walk of the workspace finds.
pub struct Found {
    pub packages: Vec<Candidate>,
    pub group_files: Vec<PathBuf>,
    /// Python nodes anywhere, also those outside every package.
    pub python_nodes: Vec<PathBuf>,
}

/// Globs where `*` stops at a slash and `**` crosses any number of directories.
pub fn glob_set(patterns: &[String], file: &Path) -> Result<GlobSet, Diag> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let g = GlobBuilder::new(p)
            .literal_separator(true)
            .build()
            .map_err(|e| Diag::file(file, format!("bad ignore glob `{p}`: {e}")))?;
        b.add(g);
    }
    b.build().map_err(|e| Diag::file(file, format!("bad ignore globs: {e}")))
}

/// Does a glob relative to `base` exclude `p`. A directory counts as excluded when its
/// contents would be, so `logs/**` prunes `logs` itself.
pub fn ignored(set: &GlobSet, base: &Path, p: &Path, is_dir: bool) -> bool {
    if set.is_empty() {
        return false;
    }
    let rel = paths::relative(p, base);
    set.is_match(&rel) || (is_dir && set.is_match(format!("{rel}/x")))
}

/// A CMake build tree, whose fetched sources mention Rant but are never packages.
fn build_tree(dir: &Path) -> bool {
    dir.join("CMakeCache.txt").is_file()
}

/// Every folder under start and every venv met on the way, which is not entered. Version
/// control, rant's data, npm trees, fetched sources, build trees and the workspace's
/// `ignore` globs are left out, gitignored paths are not, since a venv usually is one.
pub fn folders(start: &Path, config: Option<&WorkspaceConfig>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let ignore = config.and_then(|c| glob_set(&c.ignore, &c.root.join(MANIFEST)).ok()).unwrap_or_else(GlobSet::empty);
    let root = config.map(|c| c.root.clone()).unwrap_or_else(|| start.to_path_buf());
    let top = start.to_path_buf();
    let (mut dirs, mut venvs) = (Vec::new(), Vec::new());
    let walker = ignore::WalkBuilder::new(start)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let p = e.path();
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            p == top || !is_dir || !(build_tree(p) || ignored(&ignore, &root, p, true) || {
                let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                name == ".git" || name == ".rant" || name == "node_modules" || name == "_deps"
            })
        })
        .build();
    for e in walker.flatten() {
        if !e.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        if e.path().join("pyvenv.cfg").is_file() {
            venvs.push(e.path().to_path_buf());
        } else if !venvs.iter().any(|v| crate::paths::within(e.path(), v)) {
            dirs.push(e.path().to_path_buf());
        }
    }
    (dirs, venvs)
}

pub fn walk(config: &WorkspaceConfig, cache: &mut Cache, diags: &mut Vec<Diag>) -> Found {
    let ignore = match glob_set(&config.ignore, &config.root.join(MANIFEST)) {
        Ok(g) => g,
        Err(d) => {
            diags.push(d);
            GlobSet::empty()
        }
    };
    let root = config.root.clone();
    let filter = ignore.clone();
    let walker = ignore::WalkBuilder::new(&config.root)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let p = e.path();
            if p == root {
                return true;
            }
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            !(is_dir && (skipped_dir(p) || build_tree(p))) && !ignored(&filter, &root, p, is_dir)
        })
        .build();

    let mut found = Found { packages: Vec::new(), group_files: Vec::new(), python_nodes: Vec::new() };
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let ext = path.extension().unwrap_or_default();
            if path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(crate::group::SUFFIX)) {
                found.group_files.push(path.to_path_buf());
            }
            if ext == "py" && entry.metadata().is_ok_and(|m| cache.classify(path, &m) == Some(NodeKind::Python)) {
                found.python_nodes.push(path.to_path_buf());
            }
            continue;
        }
        let is_root = path == config.root;
        let block = match package::read(path, &config.root, is_root) {
            Ok(b) => b,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        let manifests = manifest::in_dir(path);
        if block.is_some() || manifests.iter().any(|m| m.uses_rant) {
            found.packages.push(Candidate { dir: path.to_path_buf(), manifests, block });
        }
    }
    found
}
