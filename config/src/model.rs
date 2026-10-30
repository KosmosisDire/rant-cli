//! The workspace model: every package with its node types, resolved once and shared by
//! build, start and every listing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::diag::Diag;
use crate::discover::{self, Candidate};
use crate::group::{self, GroupFile};
use crate::manifest::{Manifest, ManifestKind};
use crate::package::{self, PackageBlock, PACKAGE_FILE};
use crate::paths;
use crate::placement::Placement;
use crate::refs;
use crate::scan::{self, Cache, NodeKind};
use crate::workspace::WorkspaceConfig;

#[derive(Debug, Clone, PartialEq)]
pub struct NodeType {
    pub package: String,
    pub name: String,
    pub kind: NodeKind,
    /// The artifact or source it runs. None for a declared node.
    pub path: Option<PathBuf>,
    pub run: Vec<String>,
    pub cwd: PathBuf,
    /// Other artifacts with the same name that lost to the newest.
    pub shadowed: Vec<PathBuf>,
    /// The file it is written in, when the manifest says: a Rust program's source.
    pub source: Option<PathBuf>,
    /// It joins the mesh once started. Only a declared program may not.
    pub expect_mesh: bool,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub block: Option<PackageBlock>,
    pub nodes: Vec<NodeType>,
    /// Put in rant's home by `rant install`, already built.
    pub installed: bool,
}

pub struct Model {
    pub config: WorkspaceConfig,
    pub packages: Vec<Package>,
    /// Python nodes outside every package, named by file alone and run from their folder.
    pub loose: Vec<NodeType>,
    pub groups: Vec<GroupFile>,
}

impl Model {
    /// The workspace's packages, then those `rant install` put in `<home>/packages` that no
    /// workspace package of the same name hides. Outside every workspace only the installed.
    pub fn load(config: WorkspaceConfig, home: Option<&Path>) -> (Model, Vec<Diag>) {
        let mut diags = Vec::new();
        let (mut packages, mut group_files, loose) = match config.home {
            true => (Vec::new(), Vec::new(), Vec::new()),
            false => workspace_packages(&config, &mut diags),
        };
        if let Some(home) = home {
            let (more, files) = installed(&home.join("packages"), &config.root, &packages, &mut diags);
            packages.extend(more);
            group_files.extend(files);
        }
        let groups = group::name_files(group_files, &packages, &mut diags);
        (Model { config, packages, loose, groups }, diags)
    }

    /// Every node type, in packages and outside them.
    pub fn node_types(&self) -> impl Iterator<Item = &NodeType> {
        self.packages.iter().flat_map(|p| p.nodes.iter()).chain(self.loose.iter())
    }

    /// Where a node lands before any group says more: the shell, the workspace, its package.
    pub fn placement_of(&self, node: &NodeType) -> Placement {
        let package = self.package(&node.package).and_then(|p| p.block.as_ref()).map(|b| b.placement.clone()).unwrap_or_default();
        Placement::from_env().inner(&self.config.placement).inner(&package)
    }

    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name).or_else(|| self.packages.iter().find(|p| refs::same_name(&p.name, name)))
    }

    pub fn package_at(&self, dir: &Path) -> Option<&Package> {
        self.packages.iter().find(|p| paths::same(&p.dir, dir))
    }

    /// The innermost package holding `p`.
    pub fn package_holding(&self, p: &Path) -> Option<&Package> {
        self.packages.iter().filter(|pkg| paths::within(p, &pkg.dir)).max_by_key(|pkg| pkg.dir.components().count())
    }
}

/// What the walk of a workspace finds: its packages, each scanned for node types, its group
/// files, and the Python nodes outside every package.
fn workspace_packages(config: &WorkspaceConfig, diags: &mut Vec<Diag>) -> (Vec<Package>, Vec<PathBuf>, Vec<NodeType>) {
    let cache_path = config.data.join("cache").join("scan.json");
    let mut cache = Cache::load(&cache_path);
    let mut found = discover::walk(config, &mut cache, diags);
    let mut candidates = without_cmake_subdirectories(std::mem::take(&mut found.packages));
    candidates.sort_by(|a, b| a.dir.cmp(&b.dir));
    let mut packages = name_packages(candidates, diags);

    let dirs: Vec<PathBuf> = packages.iter().map(|p| p.dir.clone()).collect();
    for pkg in &mut packages {
        let nested: Vec<PathBuf> = dirs.iter().filter(|d| **d != pkg.dir && paths::within(d, &pkg.dir)).cloned().collect();
        let ignore = match &pkg.block {
            Some(b) => discover::glob_set(&b.ignore, &b.file).unwrap_or_else(|d| {
                diags.push(d);
                GlobSet::empty()
            }),
            None => GlobSet::empty(),
        };
        let scanned = scan::scan(&pkg.dir, &nested, &ignore, &mut cache);
        pkg.nodes = node_types(pkg, scanned, &config.root);
    }
    cache.save(&cache_path);
    let mut loose: Vec<NodeType> = found
        .python_nodes
        .into_iter()
        .filter(|f| !dirs.iter().any(|d| paths::within(f, d)))
        .map(|f| loose_node(f, &config.root))
        .collect();
    loose.sort_by(|a, b| a.path.cmp(&b.path));
    (packages, found.group_files, loose)
}

/// The packages `rant install` put in `dir`, a folder each named after the package, with the
/// nodes its package.rant declares and the groups beside it. One in `shown` hides its namesake.
fn installed(dir: &Path, root: &Path, shown: &[Package], diags: &mut Vec<Diag>) -> (Vec<Package>, Vec<PathBuf>) {
    let (mut packages, mut group_files) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else { return (packages, group_files) };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for pkg_dir in dirs {
        let name = pkg_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || shown.iter().any(|p| p.name == name) {
            continue;
        }
        let block = match package::read(&pkg_dir, root) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(d) => {
                diags.push(d);
                continue;
            }
        };
        let mut pkg = Package { name, dir: pkg_dir.clone(), manifests: Vec::new(), block: Some(block), nodes: Vec::new(), installed: true };
        pkg.nodes = declared(&pkg).collect();
        let mut runs: Vec<PathBuf> = std::fs::read_dir(&pkg_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(group::SUFFIX)))
            .collect();
        runs.sort();
        group_files.extend(runs);
        packages.push(pkg);
    }
    (packages, group_files)
}

/// A CMake folder inside another CMake package is part of it, since CMake nests projects
/// through add_subdirectory and the parent builds the child. A `package.rant` makes a
/// folder its own package whatever its manifests say.
fn without_cmake_subdirectories(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let cmake = |c: &Candidate| c.manifests.iter().any(|m| m.kind == ManifestKind::CMake);
    let parents: Vec<PathBuf> = candidates.iter().filter(|c| cmake(c)).map(|c| c.dir.clone()).collect();
    candidates
        .into_iter()
        .filter(|c| {
            let only_cmake = c.block.is_none() && c.manifests.iter().filter(|m| m.uses_rant).all(|m| m.kind == ManifestKind::CMake);
            let nested = parents.iter().any(|p| *p != c.dir && paths::within(&c.dir, p));
            !(only_cmake && nested)
        })
        .collect()
}

/// Names each package: `name` in package.rant, else the manifest's, else the directory's.
/// Two packages with one name are an error, and the second is dropped.
fn name_packages(candidates: Vec<Candidate>, diags: &mut Vec<Diag>) -> Vec<Package> {
    let mut out: Vec<Package> = Vec::new();
    for c in candidates {
        let name = c
            .block
            .as_ref()
            .and_then(|b| b.name.clone())
            .or_else(|| c.manifests.iter().find_map(|m| m.name.clone()))
            .unwrap_or_else(|| c.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        if let Some(other) = out.iter().find(|p| p.name == name) {
            let file = c.block.as_ref().map(|b| b.file.clone()).or_else(|| c.manifests.first().map(|m| m.path.clone()));
            diags.push(Diag::file(
                &file.unwrap_or_else(|| c.dir.join(PACKAGE_FILE)),
                format!("package name `{name}` is also used by {}, set `name` in package.rant to tell them apart", other.dir.display()),
            ));
            continue;
        }
        out.push(Package { name, dir: c.dir, manifests: c.manifests, block: c.block, nodes: Vec::new(), installed: false });
    }
    out
}

/// A node type is named after its file, except a Python `main.py` or `__main__.py`, which
/// is named after its folder, so `pick/main.py` is the node type `pick`.
fn node_name(path: &Path, kind: NodeKind) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if kind == NodeKind::Python && (stem == "main" || stem == "__main__") {
        if let Some(dir) = path.parent().and_then(|d| d.file_name()) {
            return dir.to_string_lossy().into_owned();
        }
    }
    stem
}

fn loose_node(path: PathBuf, root: &Path) -> NodeType {
    let dir = path.parent().unwrap_or(root).to_path_buf();
    let mut run = python(None, &dir, root);
    run.push(slash(&path));
    NodeType {
        package: String::new(),
        name: node_name(&path, NodeKind::Python),
        kind: NodeKind::Python,
        path: Some(path),
        run,
        cwd: dir,
        shadowed: Vec::new(),
        source: None,
        expect_mesh: true,
    }
}

/// One node type per name: the newest artifact wins and the rest are kept as shadowed.
/// A declared node replaces a found one of the same name.
fn node_types(pkg: &Package, mut found: Vec<scan::Found>, root: &Path) -> Vec<NodeType> {
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    let mut by_name: BTreeMap<String, NodeType> = BTreeMap::new();
    for f in found {
        let name = node_name(&f.path, f.kind);
        if let Some(existing) = by_name.get_mut(&name) {
            existing.shadowed.push(f.path);
            continue;
        }
        let run = match f.kind {
            NodeKind::Python => {
                let mut r = python(pkg.block.as_ref(), f.path.parent().unwrap_or(&pkg.dir), root);
                r.push(slash(&f.path));
                r
            }
            _ => vec![slash(&f.path)],
        };
        by_name.insert(
            name.clone(),
            NodeType { package: pkg.name.clone(), name, kind: f.kind, path: Some(f.path), run, cwd: pkg.dir.clone(), shadowed: Vec::new(), source: None, expect_mesh: true },
        );
    }
    for m in pkg.manifests.iter().filter(|m| m.kind == ManifestKind::CSharp && m.program && m.uses_rant) {
        let Some(name) = m.name.clone() else { continue };
        let (path, run) = match dotnet_program(&pkg.dir, &name) {
            Some((path, run)) => (path, run),
            None => (m.path.clone(), Vec::new()),
        };
        by_name.insert(
            name.clone(),
            NodeType { package: pkg.name.clone(), name, kind: NodeKind::CSharp, path: Some(path), run, cwd: pkg.dir.clone(), shadowed: Vec::new(), source: None, expect_mesh: true },
        );
    }
    let target = cargo_target(&pkg.dir, root);
    for m in pkg.manifests.iter().filter(|m| m.kind == ManifestKind::Rust && m.uses_rant) {
        for (name, source) in &m.bins {
            let (path, run) = match newest_program(&target, name) {
                Some(exe) => (exe.clone(), vec![slash(&exe)]),
                None => (source.clone(), Vec::new()),
            };
            let node = NodeType {
                package: pkg.name.clone(),
                name: name.clone(),
                kind: NodeKind::Rust,
                path: Some(path),
                run,
                cwd: pkg.dir.clone(),
                shadowed: Vec::new(),
                source: Some(source.clone()),
                expect_mesh: true,
            };
            by_name.insert(name.clone(), node);
        }
    }
    for d in declared(pkg) {
        by_name.insert(d.name.clone(), d);
    }
    by_name.into_values().collect()
}

/// The nodes package.rant declares, programs rant cannot find itself.
fn declared(pkg: &Package) -> impl Iterator<Item = NodeType> + '_ {
    pkg.block.iter().flat_map(|b| b.nodes.iter()).map(|d| NodeType {
        package: pkg.name.clone(),
        name: d.name.clone(),
        kind: NodeKind::Declared,
        path: None,
        run: d.run.clone(),
        cwd: pkg.dir.clone(),
        shadowed: Vec::new(),
        source: None,
        expect_mesh: d.expect_mesh,
    })
}

/// Where cargo builds a package: CARGO_TARGET_DIR, else the target folder of the Cargo
/// workspace around the package, up to the Rant workspace root, else its own.
pub fn cargo_target(dir: &Path, root: &Path) -> PathBuf {
    if let Some(t) = std::env::var_os("CARGO_TARGET_DIR").filter(|t| !t.is_empty()) {
        return PathBuf::from(t);
    }
    for d in dir.ancestors() {
        let text = std::fs::read_to_string(d.join("Cargo.toml")).unwrap_or_default();
        if toml::from_str::<toml::Table>(&text).is_ok_and(|doc| doc.contains_key("workspace")) {
            return d.join("target");
        }
        if paths::same(d, root) {
            break;
        }
    }
    dir.join("target")
}

/// The newest build of a Cargo program, debug or release, None until it has been built.
fn newest_program(target: &Path, name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    ["debug", "release"]
        .iter()
        .map(|profile| target.join(profile).join(&file))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// The newest build of a C# program under the package's bin/: its apphost when there is one,
/// else its dll through `dotnet`. None until it has been built.
fn dotnet_program(dir: &Path, name: &str) -> Option<(PathBuf, Vec<String>)> {
    let dll_name = format!("{name}.dll");
    let newest = ignore::WalkBuilder::new(dir.join("bin"))
        .standard_filters(false)
        .build()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&dll_name))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.into_path())))
        .max_by_key(|(t, _)| *t)?;
    let dll = newest.1;
    let apphost = dll.with_file_name(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    if apphost.is_file() {
        Some((apphost.clone(), vec![slash(&apphost)]))
    } else {
        Some((dll.clone(), vec!["dotnet".into(), slash(&dll)]))
    }
}

fn slash(p: &Path) -> String {
    crate::eval::slash(p)
}

/// The interpreter for a Python node: the package's `python`, else the nearest virtualenv
/// from the source directory up to the workspace root, else the system Python. On Windows
/// that is the `python` on PATH, where `pip install` put the packages, before `py -3`,
/// which may pick another installed version.
pub fn python(block: Option<&PackageBlock>, source_dir: &Path, root: &Path) -> Vec<String> {
    if let Some(p) = block.and_then(|b| b.python.as_ref()) {
        return vec![slash(p)];
    }
    if let Some(v) = nearest_venv(source_dir, Some(root)) {
        return vec![slash(&venv_python(&v))];
    }
    if cfg!(windows) {
        match python_on_path() {
            Some(p) => vec![slash(&p)],
            None => vec!["py".into(), "-3".into()],
        }
    } else {
        vec!["python3".into()]
    }
}

/// The nearest `.venv` or `venv` holding a pyvenv.cfg, from dir up to root, or to the top
/// of the filesystem without one.
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
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// python.exe on PATH, passing over the WindowsApps stub that only opens the Store.
fn python_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.to_string_lossy().contains("WindowsApps"))
        .map(|d| d.join("python.exe"))
        .find(|p| p.is_file())
}
