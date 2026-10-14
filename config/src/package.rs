//! A package's `package.rant`.

use std::path::{Path, PathBuf};

use crate::argv;
use crate::diag::Diag;
use crate::eval::{self, Scope, StringOrList};
use crate::placement::{self, Placement};
use crate::source::{one_label, Fields, Source};

/// The file that makes its folder a package and sets how it builds and runs.
pub const PACKAGE_FILE: &str = "package.rant";

#[derive(Debug, Clone, Default)]
pub struct PackageBlock {
    pub file: PathBuf,
    pub name: Option<String>,
    /// Commands run in order, replacing the default build. None keeps the default.
    pub build: Option<Vec<Vec<String>>>,
    pub depend: Vec<String>,
    pub ignore: Vec<String>,
    pub python: Option<PathBuf>,
    pub nodes: Vec<DeclaredNode>,
    pub placement: Placement,
}

#[derive(Debug, Clone)]
pub struct DeclaredNode {
    pub name: String,
    pub run: Vec<String>,
}

/// Reads dir/package.rant when there is one.
pub fn read(dir: &Path, workspace: &Path) -> Result<Option<PackageBlock>, Diag> {
    let path = dir.join(PACKAGE_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let src = Source::read(&path)?;
    let scope = Scope { params: None, file_dir: dir, package_dir: Some(dir), workspace };
    let keys = [&["name", "build", "depend", "ignore", "python"][..], &placement::KEYS[..]].concat();
    let fields = Fields::of_body(&src, &src.body, PACKAGE_FILE, &keys, &["node"])?;

    let mut out = PackageBlock { file: path.clone(), placement: Placement::read(&src, &fields.attrs, &scope)?, ..Default::default() };
    if let Some(a) = fields.attr("name") {
        out.name = Some(eval::string(&src, a, &scope)?);
    }
    if let Some(a) = fields.attr("build") {
        let commands = match eval::string_or_list(&src, a, &scope)? {
            StringOrList::One(s) => vec![s],
            StringOrList::List(l) => l,
        };
        let split: Result<Vec<_>, _> = commands.iter().map(|c| argv::split(c)).collect();
        out.build = Some(split.map_err(|e| src.diag_at(&a.value, format!("`build`: {e}")))?);
    }
    if let Some(a) = fields.attr("depend") {
        out.depend = eval::string_list(&src, a, &scope)?;
    }
    if let Some(a) = fields.attr("ignore") {
        out.ignore = eval::string_list(&src, a, &scope)?;
    }
    if let Some(a) = fields.attr("python") {
        out.python = Some(dir.join(eval::string(&src, a, &scope)?));
    }
    for node in fields.blocks("node") {
        let name = one_label(&src, node)?.to_string();
        let nf = Fields::of(&src, node, &["run"], &[])?;
        let Some(run) = nf.attr("run") else {
            return Err(src.diag_at(node, format!("node `{name}` needs `run`")));
        };
        let run = match eval::string_or_list(&src, run, &scope)? {
            StringOrList::One(s) => argv::split(&s).map_err(|e| src.diag_at(&run.value, format!("`run`: {e}")))?,
            StringOrList::List(l) => l,
        };
        if run.is_empty() {
            return Err(src.diag_at(node, format!("node `{name}` has an empty `run`")));
        }
        if out.nodes.iter().any(|n: &DeclaredNode| n.name == name) {
            return Err(src.diag_at(node, format!("node `{name}` is declared twice")));
        }
        out.nodes.push(DeclaredNode { name, run });
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn reads_every_field() {
        let t = TestDir::new();
        t.write(
            "dash/package.rant",
            r#"name   = "dashboard"
build  = ["npm ci", "npm run build"]
depend = ["rant_msgs"]
ignore = ["**/calib"]
python = ".venv/bin/python"
node "dashboard" {
  run = "npm run serve -- --port '80 80'"
}
node "raw" {
  run = ["${path.package}/x", "a b"]
}
"#,
        );
        let p = read(&t.path("dash"), t.root()).unwrap().unwrap();
        assert_eq!(p.name.as_deref(), Some("dashboard"));
        assert_eq!(p.build.unwrap(), vec![vec!["npm", "ci"], vec!["npm", "run", "build"]]);
        assert_eq!(p.depend, ["rant_msgs"]);
        assert_eq!(p.nodes[0].run, ["npm", "run", "serve", "--", "--port", "80 80"]);
        assert!(p.nodes[1].run[0].ends_with("/dash/x"));
        assert_eq!(p.nodes[1].run[1], "a b");
    }

    #[test]
    fn node_needs_run() {
        let t = TestDir::new();
        t.write("p/package.rant", "name = \"p\"\nnode \"x\" {}\n");
        let err = read(&t.path("p"), t.root()).err().unwrap();
        assert!(err.message.contains("needs `run`"));
        assert_eq!(err.line, 2);
    }
}
