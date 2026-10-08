//! The template.hcl of a `rant new` template: its description, the templates it includes,
//! the next steps it suggests, and its params in the same `param` blocks groups use. The
//! C++ side finds and renders the files.

use std::collections::BTreeMap;
use std::path::Path;

use hcl::Value;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::param::{self, Param};
use crate::source::{no_labels, Fields, Source};

pub struct Manifest {
    pub description: Option<String>,
    pub include: Vec<String>,
    pub next: Option<String>,
    pub params: Vec<Param>,
}

/// Reads a template.hcl from its text. path names it in errors.
pub fn read(text: &str, path: &Path) -> Result<Manifest, Diag> {
    let src = Source::parse(path, text.to_string())?;
    let blocks = src.top_blocks()?;
    let block = match blocks.as_slice() {
        [b] if b.ident.as_str() == "template" => *b,
        _ => return Err(Diag::file(path, "a template.hcl holds exactly one `template` block")),
    };
    no_labels(&src, block)?;
    let fields = Fields::of(&src, block, &["description", "include", "next"], &["param"])?;
    let empty = BTreeMap::new();
    let dir = src.dir().to_path_buf();
    let scope = Scope { params: Some(&empty), file_dir: &dir, package_dir: None, workspace: &dir };
    let params = param::parse_all(&src, &fields, &scope)?;
    if let Some(p) = params.iter().find(|p| p.name == "name") {
        return Err(p.at.diag("`name` is the name given to `rant new`, a param cannot take it"));
    }
    Ok(Manifest {
        description: fields.attr("description").map(|a| eval::string(&src, a, &scope)).transpose()?,
        include: fields.attr("include").map(|a| eval::string_list(&src, a, &scope)).transpose()?.unwrap_or_default(),
        next: fields.attr("next").map(|a| eval::string(&src, a, &scope)).transpose()?,
        params,
    })
}

/// Every param's value from `key=value` words, checked, defaults filled in, as JSON.
pub fn bind(m: &Manifest, raw: &[String]) -> Result<String, Diag> {
    let mut given = BTreeMap::new();
    for kv in raw {
        let (k, v) = kv.split_once('=').ok_or_else(|| Diag::plain(format!("`{kv}` is not key=value")))?;
        let p = m.params.iter().find(|p| p.name == k).ok_or_else(|| Diag::plain(format!("the template has no param `{k}`")))?;
        given.insert(k.to_string(), param::parse_value(p, v).map_err(Diag::plain)?);
    }
    let values: BTreeMap<String, Value> = param::bind(&m.params, given, &|m| Diag::plain(m), "")?;
    serde_json::to_string(&values).map_err(|e| Diag::plain(format!("the params cannot be written: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "template {\n  description = \"A web panel\"\n  include = [\"node-cpp\"]\n  next = \"open port {{ port }}\"\n  param \"port\" {\n    type = int\n  }\n  param \"theme\" {\n    default = \"dark\"\n    options = [\"dark\", \"light\"]\n  }\n}\n";

    #[test]
    fn a_manifest_reads_and_binds_its_params() {
        let m = read(MANIFEST, Path::new("tpl/template.hcl")).unwrap();
        assert_eq!(m.description.as_deref(), Some("A web panel"));
        assert_eq!(m.include, ["node-cpp"]);
        assert_eq!(m.params.len(), 2);
        assert_eq!(bind(&m, &["port=8080".into()]).unwrap(), r#"{"port":8080,"theme":"dark"}"#);
        assert!(bind(&m, &[]).unwrap_err().message.contains("required param `port`"));
        assert!(bind(&m, &["port=eighty".into()]).unwrap_err().message.contains("takes an int"));
        assert!(bind(&m, &["port=1".into(), "theme=pink".into()]).unwrap_err().message.contains("must be one of"));
        assert!(bind(&m, &["colour=red".into()]).unwrap_err().message.contains("no param `colour`"));
    }

    #[test]
    fn a_param_cannot_take_the_name() {
        let text = "template {\n  param \"name\" {}\n}\n";
        assert!(read(text, Path::new("t.hcl")).err().unwrap().message.contains("`name` is the name"));
        assert!(read("other {}\n", Path::new("t.hcl")).is_err());
    }
}
