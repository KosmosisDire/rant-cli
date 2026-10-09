//! Where a node lands on the mesh: its domain, the prefix of its node name and the prefix
//! of every name it creates. The shell, the workspace, the node's package, its groups from
//! outer to inner and the node itself each may set them, in that order. Prefixes join along
//! the way and the innermost domain wins. The Rant library reads the result from the
//! RANT_DOMAIN and RANT_PREFIX a node starts with, and the node prefix goes into its name.

use std::collections::BTreeMap;

use hcl::Value;
use hcl_edit::structure::Attribute;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::source::Source;

/// The attributes that set a placement, wherever one may be set.
pub const KEYS: [&str; 3] = ["domain", "prefix", "node_prefix"];

/// What the Rant library allows: a node name of 32 bytes, and a prefix that leaves room in a
/// 64 byte name for a `/` and one byte.
const NODE_NAME_MAX: usize = 32;
const PREFIX_MAX: usize = 62;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Placement {
    pub domain: Option<u16>,
    pub prefix: Vec<String>,
    pub node_prefix: Vec<String>,
}

impl Placement {
    /// The prefixes the shell already sets, outside everything a config adds.
    pub fn from_env() -> Placement {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty()).into_iter().collect();
        Placement { domain: None, prefix: var("RANT_PREFIX"), node_prefix: var("RANT_NODE_NAME_PREFIX") }
    }

    /// The placement attributes among attrs, evaluated in scope and checked as Rant would.
    pub fn read(src: &Source, attrs: &[&Attribute], scope: &Scope) -> Result<Placement, Diag> {
        let mut out = Placement::default();
        for a in attrs {
            match a.key.as_str() {
                "domain" => {
                    let v = eval::value(src, a, scope)?;
                    let d = match &v {
                        Value::Number(n) => n.as_u64().filter(|d| *d <= 65535),
                        _ => None,
                    };
                    out.domain = Some(d.ok_or_else(|| src.diag_at(&a.value, "`domain` is a number from 0 to 65535"))? as u16);
                }
                key @ ("prefix" | "node_prefix") => {
                    let p = eval::string(src, a, scope)?;
                    if p.is_empty() || p.contains('@') || p.starts_with('/') || p.ends_with('/') {
                        return Err(src.diag_at(&a.value, format!("`{key}` cannot be empty, contain @, or start or end with /")));
                    }
                    if key == "prefix" {
                        out.prefix.push(p);
                    } else {
                        out.node_prefix.push(p);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// This placement with one set further in.
    pub fn inner(&self, inside: &Placement) -> Placement {
        Placement {
            domain: inside.domain.or(self.domain),
            prefix: self.prefix.iter().chain(&inside.prefix).cloned().collect(),
            node_prefix: self.node_prefix.iter().chain(&inside.node_prefix).cloned().collect(),
        }
    }

    /// A node's name on the mesh, its node prefix in front, and the variables that place
    /// it. The launcher passes the whole name, so the node prefix is in it already.
    pub fn apply(&self, name: &str, env: &mut BTreeMap<String, String>) -> Result<String, String> {
        let full = self.node_prefix.iter().map(String::as_str).chain(std::iter::once(name)).collect::<Vec<_>>().join("/");
        if full.len() > NODE_NAME_MAX {
            return Err(format!("the node name `{full}` is longer than Rant's {NODE_NAME_MAX} bytes"));
        }
        if !self.prefix.is_empty() {
            let prefix = self.prefix.join("/");
            if prefix.len() > PREFIX_MAX {
                return Err(format!("the name prefix `{prefix}` is longer than Rant's {PREFIX_MAX} bytes"));
            }
            env.insert("RANT_PREFIX".into(), prefix);
        }
        if let Some(d) = self.domain {
            env.insert("RANT_DOMAIN".into(), d.to_string());
        }
        Ok(full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_join_outer_to_inner_and_the_inner_domain_wins() {
        let ws = Placement { domain: Some(5), prefix: vec!["cellA".into()], node_prefix: vec![] };
        let group = Placement { domain: Some(7), prefix: vec!["left".into()], node_prefix: vec!["left".into()] };
        let p = ws.inner(&group);
        let mut env = BTreeMap::new();
        assert_eq!(p.apply("arm", &mut env).unwrap(), "left/arm");
        assert_eq!(env["RANT_PREFIX"], "cellA/left");
        assert_eq!(env["RANT_DOMAIN"], "7");
        let mut none = BTreeMap::new();
        assert_eq!(Placement::default().apply("arm", &mut none).unwrap(), "arm");
        assert!(none.is_empty(), "nothing set, nothing passed");
        let long = Placement { node_prefix: vec!["a_very_long_cell_name".into()], ..Default::default() };
        assert!(long.apply("and_a_long_node", &mut none).unwrap_err().contains("longer than"));
    }
}
