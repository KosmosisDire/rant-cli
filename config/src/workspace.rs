use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::source::{no_labels, Fields, Source};

pub const MANIFEST: &str = "rant.hcl";

/// The `workspace {}` block with every default applied.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub root: PathBuf,
    pub logs: PathBuf,
    pub ignore: Vec<String>,
}

/// Walks up from `start` to the nearest `rant.hcl` holding a `workspace {}` block. A
/// `rant.hcl` with only a `package {}` block is passed over.
pub fn open(start: &Path) -> Result<Option<WorkspaceConfig>, Diag> {
    let start = std::path::absolute(start).map_err(|e| Diag::file(start, format!("bad directory: {e}")))?;
    for dir in start.ancestors() {
        let manifest = dir.join(MANIFEST);
        if !manifest.is_file() {
            continue;
        }
        let src = Source::read(&manifest)?;
        if let Some(ws) = parse(&src, dir)? {
            return Ok(Some(ws));
        }
    }
    Ok(None)
}

fn parse(src: &Source, dir: &Path) -> Result<Option<WorkspaceConfig>, Diag> {
    let blocks = src.top_blocks()?;
    check_kinds(src, &blocks)?;
    let Some(block) = blocks.iter().find(|b| b.ident.as_str() == "workspace") else {
        return Ok(None);
    };
    no_labels(src, block)?;
    let has_package = blocks.iter().any(|b| b.ident.as_str() == "package");
    let scope = Scope {
        params: None,
        file_dir: dir,
        package_dir: has_package.then_some(dir),
        workspace: dir,
    };
    let fields = Fields::of(src, block, &["logs", "ignore"], &[])?;
    let logs = match fields.attr("logs") {
        Some(a) => dir.join(eval::string(src, a, &scope)?),
        None => dir.join("logs"),
    };
    let ignore = match fields.attr("ignore") {
        Some(a) => eval::string_list(src, a, &scope)?,
        None => Vec::new(),
    };
    Ok(Some(WorkspaceConfig { root: dir.to_path_buf(), logs, ignore }))
}

/// A `rant.hcl` holds `workspace {}`, `package {}` or both, once each.
pub fn check_kinds(src: &Source, blocks: &[&hcl_edit::structure::Block]) -> Result<(), Diag> {
    let mut seen: Vec<&str> = Vec::new();
    for b in blocks {
        let ident = b.ident.as_str();
        match ident {
            "workspace" | "package" => {
                if seen.contains(&ident) {
                    return Err(src.diag_at(*b, format!("a rant.hcl holds one `{ident}` block")));
                }
                seen.push(ident);
            }
            "group" => return Err(src.diag_at(*b, "a group lives in its own file, not in rant.hcl")),
            other => return Err(src.diag_at(*b, format!("unknown block `{other}`, expected workspace or package"))),
        }
    }
    Ok(())
}

const TEMPLATE: &str = "workspace {\n  # logs   = \"logs\"\n  # ignore = [\"experiments/**\"]\n}\n";

/// Writes a fresh `rant.hcl` into `dir`. Refused when one is already there.
pub fn init(dir: &Path) -> Result<PathBuf, Diag> {
    let dir = std::path::absolute(dir).map_err(|e| Diag::file(dir, format!("bad directory: {e}")))?;
    let manifest = dir.join(MANIFEST);
    if manifest.exists() {
        return Err(Diag::file(&manifest, "already exists"));
    }
    std::fs::write(&manifest, TEMPLATE).map_err(|e| Diag::file(&manifest, format!("cannot write: {e}")))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn finds_the_nearest_workspace_above() {
        let t = TestDir::new();
        t.write("rant.hcl", "workspace {\n  logs = \"out\"\n}\n");
        t.write("a/rant.hcl", "package {}\n");
        t.mkdir("a/b");
        let ws = open(&t.path("a/b")).unwrap().unwrap();
        assert_eq!(ws.root, t.path(""));
        assert_eq!(ws.logs, t.path("out"));
    }

    #[test]
    fn no_workspace() {
        let t = TestDir::new();
        assert!(open(&t.path("")).unwrap().is_none());
    }

    #[test]
    fn group_in_rant_hcl_is_an_error() {
        let t = TestDir::new();
        t.write("rant.hcl", "workspace {}\ngroup {}\n");
        let err = open(&t.path("")).err().unwrap();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn init_writes_a_workspace_once() {
        let t = TestDir::new();
        init(&t.path("")).unwrap();
        assert!(open(&t.path("")).unwrap().is_some());
        assert!(init(&t.path("")).is_err());
    }
}
