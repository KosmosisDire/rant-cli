//! A workspace's services: `service "Hmi" { project = "hmi" }` blocks in workspace.rant, or
//! outside every workspace in services.rant in rant's home. A bare `rant run` starts them,
//! and so does every boot once `rant enable` enabled the workspace.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;

use hcl::{Expression, Value};
use hcl_edit::structure::{Block, Structure};
use hcl_edit::Span;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::group::{GroupDef, Groups};
use crate::model::Model;
use crate::param::{self, ParamType};
use crate::paths;
use crate::source::{one_label, Fields, Loc, Source};

/// The file in rant's home that lists its services.
pub const HOME_FILE: &str = "services.rant";

pub const BLOCK: &str = "service";

/// One service block, its values read but checked against its group only when it is used.
#[derive(Debug, Clone)]
pub struct Service {
    /// The group as the block names it.
    pub group: String,
    values: Vec<(String, Value, Loc)>,
    at: Loc,
}

/// A service as `rant run` takes it in the folder of its file.
pub struct Checked {
    /// The group in full.
    pub full: String,
    /// Its params as key=value words, in the block's order.
    pub words: Vec<String>,
    /// Every param the run resolves, defaults included, which tells two runs of a group apart.
    pub values: BTreeMap<String, Value>,
}

/// The service blocks among a file's fields.
pub fn read_all(src: &Source, fields: &Fields, scope: &Scope) -> Result<Vec<Service>, Diag> {
    fields.blocks(BLOCK).map(|b| read(src, b, scope)).collect()
}

fn read(src: &Source, b: &Block, scope: &Scope) -> Result<Service, Diag> {
    let group = one_label(src, b)?.to_string();
    let mut values: Vec<(String, Value, Loc)> = Vec::new();
    for s in b.body.iter() {
        match s {
            Structure::Block(inner) => return Err(src.diag_at(inner, "a service holds only param values of its group")),
            Structure::Attribute(a) => {
                let key = a.key.as_str();
                if values.iter().any(|(k, ..)| k == key) {
                    return Err(src.diag_at(a, format!("`{key}` is set twice")));
                }
                values.push((key.to_string(), eval::value(src, a, scope)?, Loc::of(src, a)));
            }
        }
    }
    Ok(Service { group, values, at: Loc::of(src, b) })
}

impl Service {
    /// The block checked as an include's values are, and planned, so a wrong one is found at
    /// its line before anything starts.
    pub fn check(&self, model: &Model, groups: &Groups) -> Result<Checked, Diag> {
        let root = &model.config.root;
        let package = model.package_holding(root).map(|p| p.name.as_str());
        let file = groups.find(&self.group, package).map_err(|m| self.at.diag(m))?;
        let def = groups.load(file)?;
        let iface = groups.interface(&def)?;
        let mut words = Vec::new();
        let mut given = BTreeMap::new();
        for (key, v, at) in &self.values {
            let p = iface.iter().find(|p| p.name == *key).ok_or_else(|| at.diag(format!("group `{}` has no param `{key}`", file.name)))?;
            let v = param::checked(p, v.clone()).map_err(|m| at.diag(m))?;
            words.push(format!("{key}={}", param::text(&v)));
            given.insert(key.clone(), param::placed(p, v, root));
        }
        let located = |d: Diag| if d.file.as_os_str().is_empty() { self.at.diag(d.message) } else { d };
        let (_, values) = groups.plan(&def, given).map_err(located)?;
        Ok(Checked { full: file.name.clone(), words, values })
    }
}

/// Adds a block for a group started with these params, read from the command line, to the
/// file that lists the services of the model's workspace, keeping the rest as written. False
/// when a block of the group with the same params is there already.
pub fn add(model: &Model, groups: &Groups, def: &GroupDef, given: Vec<(String, Value)>) -> Result<bool, Diag> {
    let (_, values) = groups.plan(def, given.iter().cloned().collect())?;
    let same = |s: &Service| s.check(model, groups).is_ok_and(|c| c.full == def.file.name && c.values == values);
    if model.config.services.iter().any(same) {
        return Ok(false);
    }
    let root = &model.config.root;
    let iface = groups.interface(def)?;
    // a path inside the workspace is written from its root, the folder the file counts from
    let written: Vec<(String, Value)> = given
        .into_iter()
        .map(|(k, v)| match (iface.iter().find(|p| p.name == k).map(|p| p.ty), v) {
            (Some(ParamType::Path), Value::String(s)) if paths::within(Path::new(&s), root) => {
                let rel = paths::relative(Path::new(&s), root);
                (k, Value::String(if rel.is_empty() { ".".into() } else { rel }))
            }
            (_, v) => (k, v),
        })
        .collect();
    let here = model.package_holding(root).map(|p| p.name.as_str());
    let file = &model.config.services_file;
    let mut text = existing(file)?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if !text.is_empty() && !text.ends_with('\n') {
        text.push_str(eol);
    }
    if !text.trim().is_empty() {
        text.push_str(eol);
    }
    text.push_str(&block_text(&groups.short_name(&def.file, here), &written, eol));
    write(file, &text)?;
    Ok(true)
}

/// A block's text, each value written as HCL reads it back, the keys lined up.
fn block_text(group: &str, values: &[(String, Value)], eol: &str) -> String {
    let label = Expression::from(group);
    if values.is_empty() {
        return format!("{BLOCK} {label} {{}}{eol}");
    }
    let width = values.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let mut out = format!("{BLOCK} {label} {{{eol}");
    for (k, v) in values {
        out += &format!("  {k:width$} = {}{eol}", Expression::from(v.clone()));
    }
    out + "}" + eol
}

/// Removes the service blocks at these places, in the order the file writes them, keeping
/// the rest as written.
pub fn remove(file: &Path, indexes: &[usize]) -> Result<(), Diag> {
    let src = Source::read(file)?;
    let spans: Vec<Range<usize>> = src
        .body
        .iter()
        .filter_map(|s| match s {
            Structure::Block(b) if b.ident.as_str() == BLOCK => b.span(),
            _ => None,
        })
        .collect();
    let mut picked = Vec::new();
    for &i in indexes {
        picked.push(spans.get(i).cloned().ok_or_else(|| Diag::file(file, format!("has no service {}", i + 1)))?);
    }
    picked.sort_by_key(|r| std::cmp::Reverse(r.start));
    picked.dedup();
    let mut text = src.text;
    for r in picked {
        cut(&mut text, r);
    }
    write(file, &text)
}

/// Cuts a block out with the lines it fills, and the blank line before it when only a blank
/// line or the end follows, so the file reads as if it was never there.
fn cut(text: &mut String, span: Range<usize>) {
    let line_start = text[..span.start].rfind('\n').map_or(0, |i| i + 1);
    let mut start = if text[line_start..span.start].trim().is_empty() { line_start } else { span.start };
    let line_end = text[span.end..].find('\n').map_or(text.len(), |i| span.end + i + 1);
    let rest = text[span.end..line_end].trim();
    let end = if rest.is_empty() || rest.starts_with('#') || rest.starts_with("//") { line_end } else { span.end };
    if start == line_start && start > 0 {
        let prev = text[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        let blank_after = text[end..].split('\n').next().is_none_or(|l| l.trim().is_empty());
        if text[prev..start].trim().is_empty() && blank_after {
            start = prev;
        }
    }
    text.replace_range(start..end, "");
}

/// A file's text, empty when it is not there yet.
fn existing(file: &Path) -> Result<String, Diag> {
    match std::fs::read_to_string(file) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(Diag::file(file, format!("cannot read: {e}"))),
    }
}

fn write(file: &Path, text: &str) -> Result<(), Diag> {
    std::fs::write(file, text).map_err(|e| Diag::file(file, format!("cannot write: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use crate::workspace;

    fn workspace_with(text: &str) -> TestDir {
        let t = TestDir::new();
        t.write("workspace.rant", text);
        t.write("hmi/package.rant", "name = \"hmi\"\nnode \"hmi\" {\n  run = \"hmi\"\n}\n");
        t.write(
            "hmi/Hmi.run",
            "param \"project\" {\n  type = path\n}\nparam \"port\" {\n  type    = int\n  default = 80\n}\nnode \"hmi\" {}\n",
        );
        t
    }

    fn model(t: &TestDir) -> Model {
        let (m, diags) = Model::load(workspace::open(t.root()).unwrap().unwrap(), None);
        assert!(diags.is_empty(), "{diags:?}");
        m
    }

    fn checked(t: &TestDir) -> Vec<Result<Checked, Diag>> {
        let m = model(t);
        let groups = Groups::new(&m);
        m.config.services.iter().map(|s| s.check(&m, &groups)).collect()
    }

    #[test]
    fn a_service_reads_as_rant_run_takes_it() {
        let t = workspace_with("logs = \"out\"\n\nservice \"Hmi\" {\n  project = \"hmi/main\"\n  port    = 81\n}\n");
        let c = checked(&t).remove(0).unwrap();
        assert_eq!(c.full, "hmi/Hmi");
        assert_eq!(c.words, ["project=hmi/main", "port=81"]);
        assert_eq!(c.values["project"], Value::from(eval::slash(&t.path("hmi/main"))));
    }

    #[test]
    fn a_wrong_service_is_found_at_its_line_only_when_checked() {
        let t = workspace_with("service \"Hmi\" {\n  project = \"p\"\n  port    = \"80\"\n}\nservice \"Hmi\" {}\nservice \"Nope\" {}\nservice \"Hmi\" {\n  colour = 1\n}\n");
        let all = checked(&t);
        let err = |i: usize| all[i].as_ref().err().unwrap().clone();
        assert_eq!((err(0).line, err(0).message.as_str()), (3, "param `port` is int, not a string"));
        assert_eq!(err(1).line, 5);
        assert!(err(1).message.starts_with("param `project` needs a value"), "{}", err(1).message);
        assert_eq!(err(2).line, 6);
        assert!(err(2).message.contains("no group named `Nope`"), "{}", err(2).message);
        assert_eq!((err(3).line, err(3).message.as_str()), (8, "group `hmi/Hmi` has no param `colour`"));
    }

    #[test]
    fn a_service_holds_only_values() {
        let t = workspace_with("service \"Hmi\" {\n  node \"x\" {}\n}\n");
        assert_eq!(workspace::open(t.root()).err().unwrap().line, 2);
        let t = workspace_with("service \"Hmi\" {\n  port = 1\n  port = 2\n}\n");
        assert_eq!(workspace::open(t.root()).err().unwrap().line, 3);
    }

    fn add_words(t: &TestDir, words: &[(&str, Value)]) -> bool {
        let m = model(t);
        let groups = Groups::new(&m);
        let def = groups.load(groups.find("Hmi", None).unwrap()).unwrap();
        add(&m, &groups, &def, words.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()).unwrap()
    }

    #[test]
    fn adding_writes_a_block_once_and_keeps_the_file() {
        let t = workspace_with("# the plant\r\nlogs = \"out\"\r\n");
        let project = Value::from(eval::slash(&t.path("hmi/main")));
        assert!(add_words(&t, &[("project", project.clone()), ("port", Value::from(81))]));
        let text = std::fs::read_to_string(t.path("workspace.rant")).unwrap();
        assert_eq!(text, "# the plant\r\nlogs = \"out\"\r\n\r\nservice \"Hmi\" {\r\n  project = \"hmi/main\"\r\n  port    = 81\r\n}\r\n");
        assert!(!add_words(&t, &[("port", Value::from(81)), ("project", project)]), "the same params");
        assert!(add_words(&t, &[("project", Value::from("C:/elsewhere/a \"b\" ${c}"))]));
        let c = checked(&t).remove(1).unwrap();
        assert_eq!(c.words, ["project=C:/elsewhere/a \"b\" ${c}"]);
    }

    #[test]
    fn removing_cuts_only_the_blocks() {
        let t = workspace_with("logs = \"out\"\n\nservice \"Hmi\" {\n  project = \"a\"\n}\n\n# the second\nservice \"Hmi\" {\n  project = \"b\"\n} # kept for b\n\nservice \"Hmi\" { project = \"c\" }\n");
        remove(&t.path("workspace.rant"), &[2, 0]).unwrap();
        let text = std::fs::read_to_string(t.path("workspace.rant")).unwrap();
        assert_eq!(text, "logs = \"out\"\n\n# the second\nservice \"Hmi\" {\n  project = \"b\"\n} # kept for b\n");
        remove(&t.path("workspace.rant"), &[0]).unwrap();
        let text = std::fs::read_to_string(t.path("workspace.rant")).unwrap();
        assert_eq!(text, "logs = \"out\"\n\n# the second\n");
        assert!(remove(&t.path("workspace.rant"), &[0]).is_err());
    }
}
