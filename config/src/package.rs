//! A package's `package.rant`.

use std::path::{Path, PathBuf};

use crate::argv;
use crate::diag::Diag;
use crate::eval::{self, FileName, Scope, StringOrList};
use crate::placement::{self, Placement};
use crate::source::{one_label, Fields, Source};

/// The file that makes its folder a package and sets how it builds and runs.
pub const PACKAGE_FILE: &str = "package.rant";

#[derive(Debug, Clone, Default)]
pub struct PackageBlock {
    pub file: PathBuf,
    pub name: Option<String>,
    /// What the package is for, shown by `rant info package`.
    pub description: Option<String>,
    /// The files its releases carry for `rant install`, by kind: an archive unpacked into
    /// rant's home, a vsix installed into editors.
    pub archive: Option<FileName>,
    pub vsix: Option<FileName>,
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
    /// False for a program that never joins the mesh, so rant run waits only for it to start.
    pub expect_mesh: bool,
}

/// Reads dir/package.rant when there is one.
pub fn read(dir: &Path, workspace: &Path) -> Result<Option<PackageBlock>, Diag> {
    let path = dir.join(PACKAGE_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    from_source(Source::read(&path)?, dir, workspace).map(Some)
}

/// Reads package.rant text as if it were in dir, such as the one of a release not yet installed.
pub fn from_text(dir: &Path, text: String, workspace: &Path) -> Result<PackageBlock, Diag> {
    from_source(Source::parse(&dir.join(PACKAGE_FILE), text)?, dir, workspace)
}

fn from_source(src: Source, dir: &Path, workspace: &Path) -> Result<PackageBlock, Diag> {
    let scope = Scope { params: None, file_dir: dir, package_dir: Some(dir), workspace };
    let keys = [&["name", "description", "archive", "vsix", "build", "depend", "ignore", "python"][..], &placement::KEYS[..]].concat();
    let fields = Fields::of_body(&src, &src.body, PACKAGE_FILE, &keys, &["node"])?;

    let mut out = PackageBlock { file: src.path.clone(), placement: Placement::read(&src, &fields.attrs, &scope)?, ..Default::default() };
    if let Some(a) = fields.attr("name") {
        out.name = Some(eval::string(&src, a, &scope)?);
    }
    if let Some(a) = fields.attr("description") {
        out.description = Some(eval::string(&src, a, &scope)?);
    }
    out.archive = fields.attr("archive").map(|a| FileName::read(&src, a)).transpose()?;
    out.vsix = fields.attr("vsix").map(|a| FileName::read(&src, a)).transpose()?;
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
        let nf = Fields::of(&src, node, &["run", "expect_mesh"], &[])?;
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
        let expect_mesh = nf.attr("expect_mesh").map(|a| eval::boolean(&src, a, &scope)).transpose()?.unwrap_or(true);
        out.nodes.push(DeclaredNode { name, run, expect_mesh });
    }
    Ok(out)
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
  run         = ["${path.package}/x", "a b"]
  expect_mesh = false
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
        assert_eq!((p.nodes[0].expect_mesh, p.nodes[1].expect_mesh), (true, false));
    }

    #[test]
    fn reads_what_its_releases_carry() {
        let t = TestDir::new();
        let text = "name = \"bridge\"\ndescription = \"Lends sockets\"\narchive = \"rant-bridge-${version}-${platform}.zip\"\n";
        let p = from_text(&t.path("bridge"), text.into(), t.root()).unwrap();
        assert_eq!(p.description.as_deref(), Some("Lends sockets"));
        let target = eval::Target { version: "0.3.0", platform: "linux-x64" };
        assert_eq!(p.archive.unwrap().of(&target).unwrap(), "rant-bridge-0.3.0-linux-x64.zip");
        assert!(p.vsix.is_none());
        assert!(p.file.ends_with("bridge/package.rant"));
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
