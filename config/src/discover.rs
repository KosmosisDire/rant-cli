//! The walk that finds packages and group files: every directory under the workspace root
//! that uses Rant, declares `package {}` or holds a Python node, and every `*.hcl` file
//! whose first block is `group`, skipping gitignored paths and the workspace `ignore` globs.

use std::io::Read;
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

/// The first top level identifier, past whitespace and comments. Only the start of a file
/// is read, so another tool's .hcl file costs almost nothing.
pub fn first_identifier(text: &str) -> Option<&str> {
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        if rest.starts_with('#') || rest.starts_with("//") {
            rest = rest.find('\n').map(|i| &rest[i..]).unwrap_or("");
        } else if let Some(body) = rest.strip_prefix("/*") {
            rest = body.find("*/").map(|i| &body[i + 2..]).unwrap_or("");
        } else {
            break;
        }
    }
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

fn is_group_file(path: &Path) -> bool {
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(4096).read_to_end(&mut head));
    read.is_ok() && first_identifier(&String::from_utf8_lossy(&head)) == Some("group")
}

/// A folder of Python scripts with no manifest still uses Rant when one of its own files is
/// a node, so the folder is a package. Its subfolders are judged on their own.
fn holds_python_node(dir: &Path, cache: &mut Cache) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let p = e.path();
        p.extension().is_some_and(|x| x == "py")
            && e.metadata().is_ok_and(|m| m.is_file() && cache.classify(&p, &m) == Some(NodeKind::Python))
    })
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

    let mut found = Found { packages: Vec::new(), group_files: Vec::new() };
    for entry in walker.flatten() {
        let path = entry.path();
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let hcl = path.extension().is_some_and(|e| e == "hcl");
            if hcl && path.file_name().is_some_and(|n| n != MANIFEST) && is_group_file(path) {
                found.group_files.push(path.to_path_buf());
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
        if block.is_some() || manifests.iter().any(|m| m.uses_rant) || holds_python_node(path, cache) {
            found.packages.push(Candidate { dir: path.to_path_buf(), manifests, block });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::first_identifier;

    #[test]
    fn first_identifier_skips_comments() {
        assert_eq!(first_identifier("# x\n// y\n/* z\n */  group {"), Some("group"));
        assert_eq!(first_identifier("\n\nresource \"a\" {}"), Some("resource"));
        assert_eq!(first_identifier("/* never closed"), None);
    }
}
