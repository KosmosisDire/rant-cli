//! The walk that finds packages, group files and Python nodes under a workspace root, past
//! gitignored paths and the workspace `ignore` globs.

use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::diag::Diag;
use crate::filter;
use crate::group;
use crate::manifest::{self, Manifest};
use crate::package::{self, PackageBlock};
use crate::paths;
use crate::scan::{Cache, NodeKind};
use crate::settings::{Settings, WORKSPACE_FILE};

/// A directory that is a package, before it is named and scanned.
pub(crate) struct Candidate {
    pub dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub block: Option<PackageBlock>,
}

/// What one walk of the workspace finds.
pub(crate) struct Found {
    pub packages: Vec<Candidate>,
    pub group_files: Vec<PathBuf>,
    /// Python nodes anywhere, also those outside every package.
    pub python_nodes: Vec<PathBuf>,
}

/// The folders under a folder and the venvs met on the way, as `rant lib` walks them.
#[derive(Debug, Default, PartialEq)]
pub struct Folders {
    pub dirs: Vec<PathBuf>,
    /// Not entered, so no folder inside one is in dirs.
    pub venvs: Vec<PathBuf>,
}

/// Every folder under start, start included, past what no walk enters and the `ignore`
/// globs. Gitignored folders count, as venvs usually are.
pub fn folders(start: &Path, settings: &Settings) -> Folders {
    let ignore = filter::glob_set(&settings.ignore, &settings.root.join(WORKSPACE_FILE)).unwrap_or_else(|_| GlobSet::empty());
    let root = settings.root.clone();
    let top = start.to_path_buf();
    let walker = ignore::WalkBuilder::new(start)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let p = e.path();
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            p == top || !is_dir || !(filter::build_tree(p) || filter::never_entered(p) || filter::ignored(&ignore, &root, p, true))
        })
        .build();
    let mut out = Folders::default();
    for e in walker.flatten() {
        if !e.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        let dir = e.path();
        if filter::is_venv(dir) {
            out.venvs.push(dir.to_path_buf());
        } else if !out.venvs.iter().any(|v| paths::within(dir, v)) {
            out.dirs.push(dir.to_path_buf());
        }
    }
    out
}

pub(crate) fn walk(settings: &Settings, cache: &mut Cache, diags: &mut Vec<Diag>) -> Found {
    let ignore = filter::glob_set(&settings.ignore, &settings.root.join(WORKSPACE_FILE)).unwrap_or_else(|d| {
        diags.push(d);
        GlobSet::empty()
    });
    let root = settings.root.clone();
    let walker = ignore::WalkBuilder::new(&settings.root)
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
            !(is_dir && (filter::skipped(p) || filter::build_tree(p))) && !filter::ignored(&ignore, &root, p, is_dir)
        })
        .build();

    let mut found = Found { packages: Vec::new(), group_files: Vec::new(), python_nodes: Vec::new() };
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if group::is_group_file(path) {
                found.group_files.push(path.to_path_buf());
            }
            let python = path.extension().is_some_and(|e| e == "py");
            if python && entry.metadata().is_ok_and(|m| cache.classify(path, &m) == Some(NodeKind::Python)) {
                found.python_nodes.push(path.to_path_buf());
            }
            continue;
        }
        if path != settings.root && path.join(WORKSPACE_FILE).is_file() {
            diags.push(Diag::file(&path.join(WORKSPACE_FILE), "a workspace inside a workspace, remove one of the two"));
            continue;
        }
        let block = match package::read(path, &settings.root) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn folders_list_venvs_without_entering_them() {
        let t = TestDir::new();
        t.write("workspace.rant", "ignore = [\"old/**\"]\n");
        t.write(".gitignore", ".venv/\n");
        t.write("app/.venv/pyvenv.cfg", "");
        t.mkdir("app/.venv/lib");
        t.mkdir("app/src");
        t.mkdir("old/stuff");
        t.mkdir("node_modules/x");
        t.write("app/build/CMakeCache.txt", "");
        let settings = Settings::open(t.root()).unwrap().unwrap();
        let f = folders(t.root(), &settings);
        let rel = |v: &[PathBuf]| v.iter().map(|p| paths::relative(p, t.root())).collect::<Vec<_>>();
        let mut dirs = rel(&f.dirs);
        dirs.sort();
        assert_eq!(dirs, ["", "app", "app/src"]);
        assert_eq!(rel(&f.venvs), ["app/.venv"]);
    }
}
