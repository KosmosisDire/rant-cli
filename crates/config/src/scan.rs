//! Finds the node types in one package: executables that carry Rant and Python files that
//! import it, cached by path, time and size so a second scan reads only what changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use globset::GlobSet;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{filter, paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Native,
    Python,
    CSharp,
    Declared,
    Rust,
}

impl NodeKind {
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Native => "native",
            NodeKind::Python => "python",
            NodeKind::CSharp => "csharp",
            NodeKind::Declared => "declared",
            NodeKind::Rust => "rust",
        }
    }
}

/// A file the scan accepted as a node.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub path: PathBuf,
    pub kind: NodeKind,
    pub modified: SystemTime,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Cache {
    version: u32,
    files: HashMap<String, Entry>,
    #[serde(skip)]
    dirty: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    modified: u128,
    size: u64,
    node: Option<NodeKind>,
}

/// Bumped whenever the rules for what is a node change, so old verdicts are dropped.
const CACHE_VERSION: u32 = 3;

impl Cache {
    pub(crate) fn load(path: &Path) -> Cache {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
            .filter(|c| c.version == CACHE_VERSION)
            .unwrap_or(Cache { version: CACHE_VERSION, ..Default::default() })
    }

    /// Writes the cache back when the scan changed it. A failure only costs a rescan.
    pub(crate) fn save(&self, path: &Path) {
        if !self.dirty {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec(self) {
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub(crate) fn classify(&mut self, path: &Path, meta: &std::fs::Metadata) -> Option<NodeKind> {
        let key = path.to_string_lossy().into_owned();
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
        if let Some(e) = self.files.get(&key)
            && e.modified == modified && e.size == meta.len() {
                return e.node;
            }
        let node = classify(path);
        self.files.insert(key, Entry { modified, size: meta.len(), node });
        self.dirty = true;
        node
    }
}

/// Scans `dir`, including build directories, but not the nested packages in `nested` nor
/// paths matching `ignore` (relative to `dir`).
pub(crate) fn scan(dir: &Path, nested: &[PathBuf], ignore: &GlobSet, cache: &mut Cache) -> Vec<Found> {
    let root = dir.to_path_buf();
    let nested: Vec<PathBuf> = nested.to_vec();
    let walk_ignore = ignore.clone();
    let self_exe = std::env::current_exe().ok();
    let walker = ignore::WalkBuilder::new(dir)
        .standard_filters(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let p = e.path();
            if p == root {
                return true;
            }
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            if is_dir && (filter::skipped(p) || nested.iter().any(|n| n == p)) {
                return false;
            }
            !filter::ignored(&walk_ignore, &root, p, is_dir)
        })
        .build();

    let mut out = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if self_exe.as_deref().is_some_and(|s| paths::same(s, path)) {
            continue; // the CLI links Rant too, but it is not a node
        }
        if !candidate(path) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if let Some(kind) = cache.classify(path, &meta) {
            out.push(Found { path: path.to_path_buf(), kind, modified: meta.modified().unwrap_or(UNIX_EPOCH) });
        }
    }
    out
}

/// Cheap checks on the name and mode, before any byte is read.
fn candidate(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    ext == "py" || executable(path, &ext)
}

#[cfg(windows)]
fn executable(_: &Path, ext: &str) -> bool {
    ext == "exe"
}

#[cfg(unix)]
fn executable(path: &Path, _: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

fn classify(path: &Path) -> Option<NodeKind> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "py" {
        let text = std::fs::read_to_string(path).ok()?;
        return python_node(path, &text).then_some(NodeKind::Python);
    }
    let bytes = std::fs::read(path).ok()?;
    native_node(&bytes).then_some(NodeKind::Native)
}

/// A Python file is a node when it imports rant and runs as a program, by a main guard or a
/// main function, or is its folder's script: `camera/camera.py`, or `main.py`.
pub(crate) fn python_node(path: &Path, text: &str) -> bool {
    static MAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?m)^\s*if\s+__name__\s*==\s*['"]__main__['"]\s*:|^def\s+main\s*\("#).unwrap());
    let stem = path.file_stem();
    let folder_script = stem.is_some_and(|s| s == "main" || s == "__main__")
        || (stem.is_some() && path.parent().and_then(|d| d.file_name()) == stem);
    imports_rant(text) && (folder_script || MAIN.is_match(text))
}

/// Python source that imports rant itself, as `import rant` or `from rant import`.
pub(crate) fn imports_rant(text: &str) -> bool {
    static IMPORT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(import\s+rant\b|from\s+rant(\.\w+)*\s+import\b)").unwrap());
    IMPORT.is_match(text)
}

/// The prefix every binary that links Rant carries. Assembled at run time, so the search
/// pattern itself never appears in this binary as one piece.
fn magic() -> Vec<u8> {
    let mut m = std::hint::black_box(b" CIGAM-TNAR\0".to_vec());
    m.reverse();
    m
}

/// An executable (not a library) that holds the Rant magic or imports the Rant library.
pub(crate) fn native_node(bytes: &[u8]) -> bool {
    use goblin::Object;
    let (executable, libs): (bool, Vec<&str>) = match Object::parse(bytes) {
        Ok(Object::Elf(e)) => (e.interpreter.is_some() || e.header.e_type == goblin::elf::header::ET_EXEC, e.libraries),
        Ok(Object::PE(p)) => (!p.is_lib, p.libraries),
        Ok(Object::Mach(goblin::mach::Mach::Binary(m))) => (m.header.filetype == goblin::mach::header::MH_EXECUTE, m.libs),
        Ok(Object::Mach(goblin::mach::Mach::Fat(_))) => (true, Vec::new()),
        _ => return false,
    };
    executable && (memchr::memmem::find(bytes, &magic()).is_some() || libs.iter().any(|l| is_rant_library(l)))
}

fn is_rant_library(lib: &str) -> bool {
    let name = lib.rsplit(['/', '\\']).next().unwrap_or(lib).to_ascii_lowercase();
    name == "rant.dll" || name.starts_with("librant.so") || (name.starts_with("librant") && name.ends_with(".dylib"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_needs_a_main_and_a_direct_import() {
        let p = Path::new("pkg/tool.py");
        assert!(python_node(p, "import rant\n\nif __name__ == \"__main__\":\n    main()\n"));
        assert!(python_node(p, "from rant import Node\n\ndef main():\n    pass\n"));
        assert!(!python_node(p, "import rant\n"));
        assert!(!python_node(p, "import grant\nif __name__ == '__main__':\n  pass\n"));
        assert!(!python_node(p, "from helpers import node\nif __name__ == '__main__':\n  pass\n"));
    }

    #[test]
    fn python_named_for_its_folder_needs_only_the_import() {
        assert!(python_node(Path::new("camera/camera.py"), "import rant\nnode = rant.Node()\n"));
        assert!(!python_node(Path::new("camera/camera.py"), "import os\n"));
        assert!(!python_node(Path::new("camera/helpers.py"), "import rant\n"));
        assert!(python_node(Path::new("detector/main.py"), "import rant\nnode = rant.Node()\n"));
    }

    #[test]
    fn the_magic_is_not_stored_whole() {
        assert_eq!(magic(), b"\0RANT-MAGIC ");
    }

    #[test]
    fn libraries_by_name() {
        assert!(is_rant_library("RANT.dll"));
        assert!(is_rant_library("librant.so.0"));
        assert!(is_rant_library("@rpath/librant.dylib"));
        assert!(!is_rant_library("libgrant.so"));
    }
}
