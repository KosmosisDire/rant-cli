//! The walk that finds packages: every directory under the workspace root that uses Rant
//! or declares `package {}`, skipping gitignored paths and the workspace `ignore` globs.

use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::diag::Diag;
use crate::manifest::{self, Manifest};
use crate::package::{self, PackageBlock};
use crate::paths;
use crate::scan::skipped_dir;
use crate::workspace::WorkspaceConfig;

/// A directory that is a package, before it is named and scanned.
pub struct Candidate {
    pub dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub block: Option<PackageBlock>,
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

pub fn candidates(config: &WorkspaceConfig, diags: &mut Vec<Diag>) -> Vec<Candidate> {
    let ignore = match glob_set(&config.ignore, &config.root.join(crate::workspace::MANIFEST)) {
        Ok(g) => g,
        Err(d) => {
            diags.push(d);
            GlobSet::empty()
        }
    };
    let root = config.root.clone();
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
            if !e.file_type().is_some_and(|t| t.is_dir()) || p == root {
                return true;
            }
            !(skipped_dir(p) || build_tree(p) || ignored(&ignore, &root, p, true))
        })
        .build();

    let mut out = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        let is_root = dir == config.root;
        let block = match package::read(dir, &config.root, is_root) {
            Ok(b) => b,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        let manifests = manifest::in_dir(dir);
        if block.is_some() || manifests.iter().any(|m| m.uses_rant) {
            out.push(Candidate { dir: dir.to_path_buf(), manifests, block });
        }
    }
    out
}
