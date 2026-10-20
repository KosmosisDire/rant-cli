use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::package::PACKAGE_FILE;
use crate::placement::{self, Placement};
use crate::source::{Fields, Source};

/// The file that makes its folder a workspace root.
pub const WORKSPACE_FILE: &str = "workspace.rant";

/// The workspace settings with every default applied.
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub root: PathBuf,
    pub logs: PathBuf,
    /// rant's own data: the state file and caches.
    pub data: PathBuf,
    pub ignore: Vec<String>,
    pub placement: Placement,
    /// No workspace: rant's home stands in, with only the installed packages.
    pub home: bool,
}

/// Walks up from `start` to the nearest folder holding a `workspace.rant`.
pub fn open(start: &Path) -> Result<Option<WorkspaceConfig>, Diag> {
    let start = std::path::absolute(start).map_err(|e| Diag::file(start, format!("bad directory: {e}")))?;
    for dir in start.ancestors() {
        let file = dir.join(WORKSPACE_FILE);
        if file.is_file() {
            return parse(&Source::read(&file)?, dir).map(Some);
        }
    }
    Ok(None)
}

fn parse(src: &Source, dir: &Path) -> Result<WorkspaceConfig, Diag> {
    let has_package = dir.join(PACKAGE_FILE).is_file();
    let scope = Scope {
        params: None,
        file_dir: dir,
        package_dir: has_package.then_some(dir),
        workspace: dir,
    };
    let keys = [&["logs", "ignore"][..], &placement::KEYS[..]].concat();
    let fields = Fields::of_body(src, &src.body, WORKSPACE_FILE, &keys, &[])?;
    let logs = match fields.attr("logs") {
        Some(a) => dir.join(eval::string(src, a, &scope)?),
        None => dir.join("logs"),
    };
    let ignore = match fields.attr("ignore") {
        Some(a) => eval::string_list(src, a, &scope)?,
        None => Vec::new(),
    };
    let placement = Placement::read(src, &fields.attrs, &scope)?;
    Ok(WorkspaceConfig { root: dir.to_path_buf(), logs, data: data_dir(dir), ignore, placement, home: false })
}

/// What stands in for a workspace outside every one: rant's home, which keeps the logs and
/// the state of what it starts.
pub fn home(rant_home: &Path) -> WorkspaceConfig {
    WorkspaceConfig {
        root: rant_home.to_path_buf(),
        logs: rant_home.join("logs"),
        data: rant_home.to_path_buf(),
        ignore: Vec::new(),
        placement: Placement::default(),
        home: true,
    }
}

/// The workspace's own data directory, `.rant/` at the root, made on first use with a
/// .gitignore so it is never committed.
pub fn data_dir(root: &Path) -> PathBuf {
    let dir = root.join(".rant");
    if !dir.is_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(".gitignore"), "*\n");
    }
    dir
}

const TEMPLATE: &str = "# logs   = \"logs\"\n# ignore = [\"experiments/**\"]\n";

/// Writes a fresh `workspace.rant` into `dir`. Refused when one is already there.
pub fn init(dir: &Path) -> Result<PathBuf, Diag> {
    let dir = std::path::absolute(dir).map_err(|e| Diag::file(dir, format!("bad directory: {e}")))?;
    let file = dir.join(WORKSPACE_FILE);
    if file.exists() {
        return Err(Diag::file(&file, "exists already"));
    }
    std::fs::write(&file, TEMPLATE).map_err(|e| Diag::file(&file, format!("cannot write: {e}")))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn finds_the_nearest_workspace_above() {
        let t = TestDir::new();
        t.write("workspace.rant", "logs = \"out\"\n");
        t.write("a/package.rant", "");
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
    fn a_block_in_workspace_rant_is_an_error() {
        let t = TestDir::new();
        t.write("workspace.rant", "logs = \"out\"\ngroup {}\n");
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
