//! Which Python a node runs with: its package's `python`, else the nearest venv, else the
//! system's.

use std::path::{Path, PathBuf};

use crate::package::PackageBlock;
use crate::paths::{self, slash};
use crate::scan;
use crate::settings::Settings;

/// The interpreter as argv, for source in `source_dir`. The venv search stops at `root`, or
/// at the top of the filesystem without one.
pub fn interpreter(block: Option<&PackageBlock>, source_dir: &Path, root: Option<&Path>) -> Vec<String> {
    match block.and_then(|b| b.python.as_ref()) {
        Some(p) => vec![slash(p)],
        None => with_venv(nearest_venv(source_dir, root).as_deref()),
    }
}

fn with_venv(venv: Option<&Path>) -> Vec<String> {
    match venv {
        Some(v) => vec![slash(&venv_python(v))],
        None => system(),
    }
}

/// On Windows the `python` on PATH, where `pip install` put the packages, before `py -3`,
/// which may pick another installed version.
fn system() -> Vec<String> {
    if !cfg!(windows) {
        return vec!["python3".into()];
    }
    match python_on_path() {
        Some(p) => vec![slash(&p)],
        None => vec!["py".into(), "-3".into()],
    }
}

/// The nearest `.venv` or `venv` holding a pyvenv.cfg, from dir up to root.
pub fn nearest_venv(dir: &Path, root: Option<&Path>) -> Option<PathBuf> {
    for d in dir.ancestors() {
        for name in [".venv", "venv"] {
            let v = d.join(name);
            if v.join("pyvenv.cfg").is_file() {
                return Some(v);
            }
        }
        if root.is_some_and(|r| paths::same(d, r)) {
            break;
        }
    }
    None
}

pub fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) { venv.join("Scripts").join("python.exe") } else { venv.join("bin").join("python") }
}

/// python.exe on PATH, passing over the WindowsApps stub that only opens the Store.
fn python_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.to_string_lossy().contains("WindowsApps"))
        .map(|d| d.join("python.exe"))
        .find(|p| p.is_file())
}

/// The Python of a folder, as a node written there would run.
#[derive(Debug, PartialEq)]
pub struct Folder {
    /// The nearest venv up to the workspace root.
    pub venv: Option<PathBuf>,
    pub interpreter: Vec<String>,
    /// One of the folder's own Python files imports rant.
    pub imports_rant: bool,
    /// The root of the workspace around it.
    pub root: Option<PathBuf>,
}

impl Folder {
    pub fn of(dir: &Path) -> Folder {
        let root = Settings::open(dir).ok().flatten().map(|s| s.root);
        let venv = nearest_venv(dir, root.as_deref());
        let imports_rant = std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
            let p = e.path();
            p.extension().is_some_and(|x| x == "py") && std::fs::read_to_string(&p).is_ok_and(|t| scan::imports_rant(&t))
        });
        Folder { interpreter: with_venv(venv.as_deref()), venv, imports_rant, root }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn a_folder_runs_with_the_nearest_venv_inside_its_workspace() {
        let t = TestDir::new();
        t.write("workspace.rant", "");
        t.write(".venv/pyvenv.cfg", "");
        t.write("arm/arm.py", "from rant import Node\n");
        t.mkdir("tools");
        let arm = Folder::of(&t.path("arm"));
        assert_eq!(arm.venv, Some(t.path(".venv")));
        assert_eq!(arm.interpreter, [slash(&venv_python(&t.path(".venv")))]);
        assert!(arm.imports_rant);
        assert_eq!(arm.root, Some(t.path("")));
        assert!(!Folder::of(&t.path("tools")).imports_rant);
    }

    #[test]
    fn the_package_python_comes_first() {
        let t = TestDir::new();
        t.write("venv/pyvenv.cfg", "");
        let block = PackageBlock { python: Some(t.path("py/bin/python")), ..Default::default() };
        assert_eq!(interpreter(Some(&block), t.root(), None), [slash(&t.path("py/bin/python"))]);
        assert_eq!(interpreter(None, t.root(), Some(t.root())), [slash(&venv_python(&t.path("venv")))]);
    }
}
