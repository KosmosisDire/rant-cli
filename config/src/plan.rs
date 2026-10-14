//! The resolved plan: the flat, ordered list of node instances a start runs, every path
//! absolute and every argument final. start, --dry-run and --help all read this one output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::group::Groups;
use crate::model::{Model, NodeType};
use crate::paths;
use crate::refs;

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// The node's name on the mesh.
    pub name: String,
    pub node: NodeType,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

impl Instance {
    /// The node type as a reference another tool can resolve again.
    pub fn type_ref(&self) -> String {
        if self.node.package.is_empty() {
            self.node.path.as_deref().map(crate::eval::slash).unwrap_or_else(|| self.node.name.clone())
        } else {
            format!("{}/{}", self.node.package, self.node.name)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub instances: Vec<Instance>,
}

/// A node type that has nothing to run yet, a C# program never built, cannot start.
pub fn runnable(node: &NodeType) -> Result<(), String> {
    if node.run.is_empty() {
        return Err(format!("`{}/{}` is not built yet, run `rant build {}` first", node.package, node.name, node.package));
    }
    Ok(())
}

/// What a name given to run, stop or restart means. Both and Neither say why.
#[derive(Debug, PartialEq)]
pub enum Named {
    Group,
    Node,
    Both(String),
    Neither(String),
}

/// A name is a group or a node type, and must not be both. A node type in several packages
/// counts, so planning it names them.
pub fn named(model: &Model, reference: &str, from_dir: &Path) -> Named {
    let package = model.package_holding(&paths::normalize(from_dir)).map(|p| p.name.clone());
    let group = Groups::new(model).find(reference, package.as_deref()).ok();
    let node = refs::resolve(model, reference, from_dir);
    let is_node = node.is_ok() || model.node_types().any(|n| n.name == reference);
    match (group, node) {
        (Some(g), _) if is_node => Named::Both(format!(
            "`{reference}` is both the group in {} and a node, rename one",
            paths::relative(&g.path, &model.config.root)
        )),
        (Some(_), _) => Named::Group,
        _ if is_node => Named::Node,
        (None, Err(e)) if reference.contains('/') || refs::is_path(reference) => Named::Neither(e),
        _ => Named::Neither(format!(
            "no group or node named `{reference}`, {}",
            refs::unbuilt(model).unwrap_or_else(|| "see `rant ls groups` and `rant ls nodes`".into())
        )),
    }
}

/// One node of a node type, named after the type, run as the type says.
pub fn single(model: &Model, reference: &str, from_dir: &Path) -> Result<Plan, Diag> {
    let node = refs::resolve(model, reference, from_dir).map_err(Diag::plain)?;
    runnable(&node).map_err(Diag::plain)?;
    let mut env = BTreeMap::new();
    let name = model.placement_of(&node).apply(&node.name, &mut env).map_err(Diag::plain)?;
    let instance = Instance { name, argv: node.run.clone(), env, cwd: node.cwd.clone(), node };
    Ok(Plan { instances: vec![instance] })
}
