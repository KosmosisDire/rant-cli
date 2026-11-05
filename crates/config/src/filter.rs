//! Which paths a walk of the workspace passes over: the `ignore` globs, and folders that hold
//! no packages or node types of their own.

use std::path::Path;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::diag::Diag;
use crate::paths;

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

/// Version control, Rant's data, npm trees, the sources CMake fetches, and caches such as a
/// cargo target folder, whose programs Cargo.toml names.
pub fn never_entered(dir: &Path) -> bool {
    let name = dir.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    matches!(name.as_ref(), ".git" | ".rant" | "node_modules" | "_deps") || dir.join("CACHEDIR.TAG").is_file()
}

pub fn is_venv(dir: &Path) -> bool {
    dir.join("pyvenv.cfg").is_file()
}

/// What no scan for packages or node types enters.
pub fn skipped(dir: &Path) -> bool {
    never_entered(dir) || is_venv(dir)
}

/// A CMake build tree, whose fetched sources mention Rant but are never packages.
pub fn build_tree(dir: &Path) -> bool {
    dir.join("CMakeCache.txt").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn a_glob_prunes_the_folder_it_empties() {
        let t = TestDir::new();
        let set = glob_set(&["logs/**".into(), "*.tmp".into()], &t.path("workspace.rant")).unwrap();
        assert!(ignored(&set, t.root(), &t.path("logs"), true));
        assert!(!ignored(&set, t.root(), &t.path("logs"), false));
        assert!(ignored(&set, t.root(), &t.path("a.tmp"), false));
        assert!(!ignored(&set, t.root(), &t.path("sub/a.tmp"), false), "`*` stops at a slash");
        assert!(glob_set(&["a[".into()], &t.path("workspace.rant")).unwrap_err().message.contains("bad ignore glob"));
    }

    #[test]
    fn folders_never_entered() {
        let t = TestDir::new();
        t.write("target/CACHEDIR.TAG", "");
        t.write(".venv/pyvenv.cfg", "");
        t.mkdir("node_modules");
        t.mkdir("src");
        assert!(never_entered(&t.path("target")) && never_entered(&t.path("node_modules")));
        assert!(!never_entered(&t.path(".venv")) && skipped(&t.path(".venv")));
        assert!(!skipped(&t.path("src")));
    }
}
