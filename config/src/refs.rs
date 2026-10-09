//! Node type references, in the three forms a group or the command line may use:
//!
//!   lidar_driver/lidar_node            a node type in a package
//!   ./tools/bin/lidar_node             an executable, relative to the referencing file
//!   ../drivers/lidar_driver:lidar_node a node type in the package at that directory
//!
//! A bare name is accepted too when exactly one package has a node type by that name.

use std::path::Path;

use crate::model::{Model, NodeType};
use crate::manifest::ManifestKind;
use crate::paths;
use crate::scan::NodeKind;

fn is_path(r: &str) -> bool {
    let b = r.as_bytes();
    r.starts_with("./")
        || r.starts_with("../")
        || r.starts_with('/')
        || (cfg!(windows) && (r.starts_with(".\\") || r.starts_with("..\\") || r.starts_with('\\')))
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\'))
}

pub fn resolve(model: &Model, reference: &str, from_dir: &Path) -> Result<NodeType, String> {
    let reference = reference.trim(); // a space at either end never means anything
    if reference.is_empty() {
        return Err("empty node name".into());
    }
    if is_path(reference) {
        if let Some((dir, name)) = reference.rsplit_once(':') {
            if !name.is_empty() && !name.contains(['/', '\\']) && dir.len() > 1 {
                return in_package_dir(model, &from_dir.join(dir), name, reference);
            }
        }
        return by_path(model, &from_dir.join(reference), reference);
    }
    if let Some((pkg, name)) = reference.split_once('/') {
        let p = model
            .package(pkg)
            .ok_or_else(|| format!("no package named `{pkg}`, see `rant ls packages`"))?;
        return p
            .nodes
            .iter()
            .find(|n| n.name == name)
            .cloned()
            .ok_or_else(|| format!("package `{pkg}` has no node `{name}`{}", known(&p.nodes)));
    }
    let matches: Vec<&NodeType> = model.node_types().filter(|n| n.name == reference).collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("no node named `{reference}`, {}", unbuilt(model))),
        many => Err(format!(
            "`{reference}` is a node in several packages, name one: {}",
            many.iter().map(|n| shown(n)).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Where to look for a node that is missing: a CMake package with no nodes yet only knows
/// them once built.
fn unbuilt(model: &Model) -> String {
    let cmake = |p: &&crate::model::Package| p.nodes.is_empty() && p.manifests.iter().any(|m| m.kind == ManifestKind::CMake);
    match model.packages.iter().find(cmake) {
        Some(p) => format!("`{}` is not built yet, run `rant build` first", p.name),
        None => "see `rant ls nodes --all`".into(),
    }
}

/// How to name one of several node types: package and name, or the file of one outside
/// every package.
fn shown(n: &NodeType) -> String {
    match (&n.path, n.package.is_empty()) {
        (Some(p), true) => paths::normalize(p).display().to_string(),
        _ => format!("{}/{}", n.package, n.name),
    }
}

fn known(nodes: &[NodeType]) -> String {
    if nodes.is_empty() {
        return ", it has none".into();
    }
    format!(", it has: {}", nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", "))
}

fn in_package_dir(model: &Model, dir: &Path, name: &str, reference: &str) -> Result<NodeType, String> {
    let p = model.package_at(dir).ok_or_else(|| format!("`{reference}`: {} is not a package", paths::normalize(dir).display()))?;
    p.nodes
        .iter()
        .find(|n| n.name == name)
        .cloned()
        .ok_or_else(|| format!("package `{}` has no node `{name}`{}", p.name, known(&p.nodes)))
}

/// An executable or Python file by path: a found node type when the scan knows it, else a
/// node type made for it on the spot.
fn by_path(model: &Model, path: &Path, reference: &str) -> Result<NodeType, String> {
    let mut path = paths::normalize(path);
    if cfg!(windows) && !path.is_file() && path.extension().is_none() {
        path.set_extension("exe");
    }
    for n in model.node_types() {
        if n.path.as_deref().is_some_and(|p| paths::same(p, &path)) {
            return Ok(n.clone());
        }
    }
    if !path.is_file() {
        return Err(format!("`{reference}`: {} does not exist", path.display()));
    }
    let owner = model.package_holding(&path);
    let python = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("py"));
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut run = if python { crate::model::python(owner.and_then(|p| p.block.as_ref()), &dir, &model.config.root) } else { Vec::new() };
    run.push(crate::eval::slash(&path));
    Ok(NodeType {
        package: owner.map(|p| p.name.clone()).unwrap_or_default(),
        name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        kind: if python { NodeKind::Python } else { NodeKind::Native },
        cwd: owner.map(|p| p.dir.clone()).unwrap_or(dir),
        path: Some(path),
        run,
        shadowed: Vec::new(),
    })
}
