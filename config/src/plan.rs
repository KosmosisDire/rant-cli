//! The resolved plan: the flat, ordered list of node instances a start runs, every path
//! absolute and every argument final. start, --dry-run and --help all read this one output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::model::{Model, NodeType};
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

/// One node of a node type, named after the type, run as the type says.
pub fn single(model: &Model, reference: &str, from_dir: &Path) -> Result<Plan, Diag> {
    let node = refs::resolve(model, reference, from_dir).map_err(Diag::plain)?;
    let instance = Instance { name: node.name.clone(), argv: node.run.clone(), env: BTreeMap::new(), cwd: node.cwd.clone(), node };
    Ok(Plan { instances: vec![instance] })
}
