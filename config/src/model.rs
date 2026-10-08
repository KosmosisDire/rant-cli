//! The workspace model: every package with its node types, resolved once and shared by
//! build, start and every listing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::GlobSet;

use crate::diag::Diag;
use crate::discover::{self, Candidate};
use crate::group::{self, GroupFile};
use crate::manifest::{Manifest, ManifestKind};
use crate::package::PackageBlock;
use crate::paths;
use crate::scan::{self, Cache, NodeKind};
use crate::workspace::{WorkspaceConfig, MANIFEST};

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
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub dir: PathBuf,
    pub manifests: Vec<Manifest>,
    pub block: Option<PackageBlock>,
    pub nodes: Vec<NodeType>,
}

pub struct Model {
    pub config: WorkspaceConfig,
    pub packages: Vec<Package>,
    pub groups: Vec<GroupFile>,
}

pub fn cache_dir(root: &Path) -> PathBuf {
    crate::workspace::data_dir(root).join("cache")
}

impl Model {
    pub fn load(config: WorkspaceConfig) -> (Model, Vec<Diag>) {
        let mut diags = Vec::new();
        let cache_path = cache_dir(&config.root).join("scan.json");
        let mut cache = Cache::load(&cache_path);
        let found = discover::walk(&config, &mut cache, &mut diags);
        let mut candidates = found.packages;
        candidates.sort_by(|a, b| a.dir.cmp(&b.dir));
        let mut packages = name_packages(candidates, &mut diags);
        let groups = group::name_files(found.group_files, &packages, &mut diags);

        let dirs: Vec<PathBuf> = packages.iter().map(|p| p.dir.clone()).collect();
        for pkg in &mut packages {
            let nested: Vec<PathBuf> =
                dirs.iter().filter(|d| **d != pkg.dir && paths::within(d, &pkg.dir)).cloned().collect();
            let ignore = match &pkg.block {
                Some(b) => discover::glob_set(&b.ignore, &b.file).unwrap_or_else(|d| {
                    diags.push(d);
                    GlobSet::empty()
                }),
                None => GlobSet::empty(),
            };
            let found = scan::scan(&pkg.dir, &nested, &ignore, &mut cache);
            pkg.nodes = node_types(pkg, found, &config.root);
        }
        cache.save(&cache_path);
        (Model { config, packages, groups }, diags)
    }

    pub fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name)
    }

    pub fn package_at(&self, dir: &Path) -> Option<&Package> {
        self.packages.iter().find(|p| paths::same(&p.dir, dir))
    }

    /// The innermost package holding `p`.
    pub fn package_holding(&self, p: &Path) -> Option<&Package> {
        self.packages.iter().filter(|pkg| paths::within(p, &pkg.dir)).max_by_key(|pkg| pkg.dir.components().count())
    }
}

/// Names each package: `package { name }`, else the manifest's, else the directory's.
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
                &file.unwrap_or_else(|| c.dir.join(MANIFEST)),
                format!("package name `{name}` is also used by {}, set `name` in a package block to tell them apart", other.dir.display()),
            ));
            continue;
        }
        out.push(Package { name, dir: c.dir, manifests: c.manifests, block: c.block, nodes: Vec::new() });
    }
    out
}

/// A node type is named after its file, except a Python `main.py` or `__main__.py`, which
/// is named after its folder, so `pick/main.py` is the node type `pick`.
fn node_name(f: &scan::Found) -> String {
    let stem = f.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if f.kind == NodeKind::Python && (stem == "main" || stem == "__main__") {
        if let Some(dir) = f.path.parent().and_then(|d| d.file_name()) {
            return dir.to_string_lossy().into_owned();
        }
    }
    stem
}

/// One node type per name: the newest artifact wins and the rest are kept as shadowed.
/// A declared node replaces a found one of the same name.
fn node_types(pkg: &Package, mut found: Vec<scan::Found>, root: &Path) -> Vec<NodeType> {
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    let mut by_name: BTreeMap<String, NodeType> = BTreeMap::new();
    for f in found {
        let name = node_name(&f);
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
            NodeType { package: pkg.name.clone(), name, kind: f.kind, path: Some(f.path), run, cwd: pkg.dir.clone(), shadowed: Vec::new() },
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
            NodeType { package: pkg.name.clone(), name, kind: NodeKind::CSharp, path: Some(path), run, cwd: pkg.dir.clone(), shadowed: Vec::new() },
        );
    }
    for d in pkg.block.iter().flat_map(|b| b.nodes.iter()) {
        by_name.insert(
            d.name.clone(),
            NodeType {
                package: pkg.name.clone(),
                name: d.name.clone(),
                kind: NodeKind::Declared,
                path: None,
                run: d.run.clone(),
                cwd: pkg.dir.clone(),
                shadowed: Vec::new(),
            },
        );
    }
    by_name.into_values().collect()
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
    for dir in source_dir.ancestors() {
        for venv in [".venv", "venv"] {
            let v = dir.join(venv);
            if v.join("pyvenv.cfg").is_file() {
                let exe = if cfg!(windows) { v.join("Scripts").join("python.exe") } else { v.join("bin").join("python") };
                return vec![slash(&exe)];
            }
        }
        if paths::same(dir, root) {
            break;
        }
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

/// python.exe on PATH, passing over the WindowsApps stub that only opens the Store.
fn python_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.to_string_lossy().contains("WindowsApps"))
        .map(|d| d.join("python.exe"))
        .find(|p| p.is_file())
}
