//! Groups: one file each, a parameterized set of nodes and included groups. A group is
//! read once, its interface (own params plus the params its includes expose) is worked
//! out, and starting it expands it depth first into a flat plan.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hcl::Value;
use hcl_edit::structure::{Attribute, Block, Structure};

use crate::argv;
use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::model::{Model, Package};
use crate::paths;
use crate::plan::{Instance, Plan};
use crate::refs;
use crate::param::{self, Param};
use crate::source::{no_labels, one_label, Fields, Loc, Source};

/// A group file and the name it goes by: `package/stem` inside a package, else `stem`.
#[derive(Debug, Clone)]
pub struct GroupFile {
    pub name: String,
    pub package: Option<String>,
    pub path: PathBuf,
}

/// Names group files after their stem and their package. Two files of one name are an
/// error, and the second is dropped.
pub fn name_files(files: Vec<PathBuf>, packages: &[Package], diags: &mut Vec<Diag>) -> Vec<GroupFile> {
    let mut out: Vec<GroupFile> = Vec::new();
    for path in files {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let package = packages
            .iter()
            .filter(|p| paths::within(&path, &p.dir))
            .max_by_key(|p| p.dir.components().count())
            .map(|p| p.name.clone());
        let name = match &package {
            Some(p) => format!("{p}/{stem}"),
            None => stem,
        };
        if let Some(other) = out.iter().find(|g| g.name == name) {
            diags.push(Diag::file(&path, format!("group `{name}` is also defined in {}", other.path.display())));
            continue;
        }
        out.push(GroupFile { name, package, path });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

enum Expose {
    None,
    All,
    Only(Vec<String>),
}

struct Include {
    group: String,
    expose: Expose,
    bindings: Vec<Attribute>,
    at: Loc,
}

enum Item {
    Include(Include),
    Node(Block),
}

/// A parsed group file. Expressions stay unevaluated until the group is expanded.
pub struct GroupDef {
    pub file: GroupFile,
    pub description: Option<String>,
    params: Vec<Param>,
    items: Vec<Item>,
    src: Source,
}

impl GroupDef {
    fn includes(&self) -> impl Iterator<Item = (usize, &Include)> {
        self.items.iter().filter_map(|i| if let Item::Include(inc) = i { Some(inc) } else { None }).enumerate()
    }
}

fn parse(file: GroupFile, root: &Path) -> Result<GroupDef, Diag> {
    let src = Source::read(&file.path)?;
    let blocks = src.top_blocks()?;
    let block = match blocks.as_slice() {
        [b] if b.ident.as_str() == "group" => *b,
        _ => return Err(Diag::file(&file.path, "a group file holds exactly one `group` block")),
    };
    no_labels(&src, block)?;
    let fields = Fields::of(&src, block, &["description"], &["param", "include", "node"])?;
    let dir = src.dir().to_path_buf();
    let empty = BTreeMap::new();
    let scope = Scope { params: Some(&empty), file_dir: &dir, package_dir: None, workspace: root };
    let description = fields.attr("description").map(|a| eval::string(&src, a, &scope)).transpose()?;

    let params = param::parse_all(&src, &fields, &scope)?;

    let mut items = Vec::new();
    for s in block.body.iter() {
        let Structure::Block(b) = s else { continue };
        match b.ident.as_str() {
            "include" => items.push(Item::Include(parse_include(&src, b, &scope)?)),
            "node" => {
                one_label(&src, b)?;
                Fields::of(&src, b, &["name", "args", "env"], &[])?;
                items.push(Item::Node(b.clone()));
            }
            _ => {}
        }
    }
    Ok(GroupDef { file, description, params, items, src })
}

fn parse_include(src: &Source, b: &Block, scope: &Scope) -> Result<Include, Diag> {
    let group = one_label(src, b)?.to_string();
    let mut inc = Include { group, expose: Expose::None, bindings: Vec::new(), at: Loc::of(src, b) };
    for s in b.body.iter() {
        match s {
            Structure::Block(inner) => return Err(src.diag_at(inner, "an include holds only param values and `expose`")),
            Structure::Attribute(a) if a.key.as_str() == "expose" => {
                inc.expose = match eval::value(src, a, scope)? {
                    Value::Bool(true) => Expose::All,
                    Value::Bool(false) => Expose::None,
                    Value::Array(_) => Expose::Only(eval::string_list(src, a, scope)?),
                    v => return Err(src.diag_at(&a.value, format!("`expose` is true, false or a list of names, not {}", eval::type_name(&v)))),
                }
            }
            Structure::Attribute(a) => inc.bindings.push(a.clone()),
        }
    }
    Ok(inc)
}

/// The groups of one workspace, each parsed on first use.
pub struct Groups<'m> {
    model: &'m Model,
    parsed: RefCell<HashMap<String, Rc<GroupDef>>>,
}

impl<'m> Groups<'m> {
    pub fn new(model: &'m Model) -> Groups<'m> {
        Groups { model, parsed: RefCell::new(HashMap::new()) }
    }

    /// A group by reference. A bare name is looked for in `package` first, then among the
    /// groups outside any package. A group in another package must be qualified.
    pub fn find(&self, reference: &str, package: Option<&str>) -> Result<&'m GroupFile, String> {
        let groups = &self.model.groups;
        if reference.contains('/') {
            return groups.iter().find(|g| g.name == reference).ok_or_else(|| self.unknown(reference));
        }
        if let Some(p) = package {
            if let Some(g) = groups.iter().find(|g| g.name == format!("{p}/{reference}")) {
                return Ok(g);
            }
        }
        if let Some(g) = groups.iter().find(|g| g.package.is_none() && g.name == reference) {
            return Ok(g);
        }
        Err(self.unknown(reference))
    }

    fn unknown(&self, reference: &str) -> String {
        let stem = reference.rsplit('/').next().unwrap_or(reference);
        let near: Vec<&str> = self.model.groups.iter().filter(|g| g.name.rsplit('/').next() == Some(stem)).map(|g| g.name.as_str()).collect();
        if near.is_empty() {
            format!("no group named `{reference}`")
        } else {
            format!("no group named `{reference}` here, did you mean {}?", near.join(" or "))
        }
    }

    pub fn load(&self, file: &GroupFile) -> Result<Rc<GroupDef>, Diag> {
        if let Some(def) = self.parsed.borrow().get(&file.name) {
            return Ok(def.clone());
        }
        let def = Rc::new(parse(file.clone(), &self.model.config.root)?);
        self.parsed.borrow_mut().insert(file.name.clone(), def.clone());
        Ok(def)
    }

    fn included(&self, def: &GroupDef, inc: &Include) -> Result<Rc<GroupDef>, Diag> {
        let file = self.find(&inc.group, def.file.package.as_deref()).map_err(|e| inc.at.diag(e))?;
        self.load(file)
    }

    /// Own params, then the params each exposing include passes up, in include order.
    pub fn interface(&self, def: &GroupDef) -> Result<Vec<Param>, Diag> {
        self.interface_in(def, &mut Vec::new())
    }

    fn interface_in(&self, def: &GroupDef, stack: &mut Vec<String>) -> Result<Vec<Param>, Diag> {
        if let Some(i) = stack.iter().position(|n| *n == def.file.name) {
            let cycle = stack[i..].join(" includes ");
            return Err(Diag::file(&def.file.path, format!("include cycle: {cycle} includes {}", def.file.name)));
        }
        stack.push(def.file.name.clone());
        let mut params = def.params.clone();
        for (idx, inc) in def.includes() {
            let child = self.included(def, inc)?;
            let child_params = self.interface_in(&child, stack)?;
            for b in &inc.bindings {
                if !child_params.iter().any(|p| p.name == b.key.as_str()) {
                    return Err(def.src.diag_at(b, format!("group `{}` has no param `{}`", child.file.name, b.key.as_str())));
                }
            }
            let unbound: Vec<&Param> = child_params.iter().filter(|p| !inc.bindings.iter().any(|b| b.key.as_str() == p.name)).collect();
            let exposed: Vec<&Param> = match &inc.expose {
                Expose::None => Vec::new(),
                Expose::All => unbound,
                Expose::Only(names) => {
                    let mut v = Vec::new();
                    for n in names {
                        match unbound.iter().find(|p| p.name == *n) {
                            Some(p) => v.push(*p),
                            None => return Err(inc.at.diag(format!("`{}` exposes `{n}`, which is not an unbound param of it", inc.group))),
                        }
                    }
                    v
                }
            };
            for p in exposed {
                if let Some(other) = params.iter().find(|q| q.name == p.name) {
                    return Err(inc.at.diag(format!("exposed param `{}` collides with the param declared at {}", p.name, other.at.shown())));
                }
                let mut p = p.clone();
                p.from = Some((child.file.name.clone(), idx));
                params.push(p);
            }
        }
        stack.pop();
        Ok(params)
    }

    /// Binds values to a group's interface: defaults fill what is not given, a required
    /// param left unset is an error, and every value matches its param. `at` places an
    /// error, and `hint` follows the message of a param left unset.
    /// The flat plan for a group started with `given`. Instances come depth first in the
    /// order the files write them, and one name twice is merged when both are the same.
    pub fn plan(&self, def: &GroupDef, given: BTreeMap<String, Value>) -> Result<(Plan, BTreeMap<String, Value>), Diag> {
        let iface = self.interface(def)?;
        for name in given.keys() {
            if !iface.iter().any(|p| p.name == *name) {
                return Err(Diag::plain(format!("group `{}` has no param `{name}`, see `rant start group {} --help`", def.file.name, def.file.name)));
            }
        }
        let help = format!(", see `rant start group {} --help`", def.file.name);
        let values = param::bind(&iface, given, &|m| Diag::plain(format!("{m}{help}")), "")?;
        let mut found = Vec::new();
        self.expand(def, &iface, &values, &mut found)?;

        let mut plan = Plan::default();
        let mut origins: Vec<Loc> = Vec::new();
        for (inst, at) in found {
            match plan.instances.iter().position(|i| i.name == inst.name) {
                Some(i) if same(&plan.instances[i], &inst) => {}
                Some(i) => {
                    return Err(at.diag(format!(
                        "node `{}` is also defined at {} with different settings",
                        inst.name,
                        origins[i].shown()
                    )))
                }
                None => {
                    plan.instances.push(inst);
                    origins.push(at);
                }
            }
        }
        Ok((plan, values))
    }

    fn expand(&self, def: &GroupDef, iface: &[Param], values: &BTreeMap<String, Value>, out: &mut Vec<(Instance, Loc)>) -> Result<(), Diag> {
        let dir = def.src.dir().to_path_buf();
        let package_dir = def.file.package.as_deref().and_then(|p| self.model.package(p)).map(|p| p.dir.clone());
        let scope = Scope { params: Some(values), file_dir: &dir, package_dir: package_dir.as_deref(), workspace: &self.model.config.root };
        let mut include_idx = 0;
        for item in &def.items {
            match item {
                Item::Include(inc) => {
                    let idx = include_idx;
                    include_idx += 1;
                    let child = self.included(def, inc)?;
                    let child_iface = self.interface(&child)?;
                    let mut given = BTreeMap::new();
                    for b in &inc.bindings {
                        given.insert(b.key.as_str().to_string(), eval::value(&def.src, b, &scope)?);
                    }
                    for p in iface.iter().filter(|p| p.from.as_ref().is_some_and(|(g, i)| *g == child.file.name && *i == idx)) {
                        given.insert(p.name.clone(), values[&p.name].clone());
                    }
                    let child_values = param::bind(
                        &child_iface,
                        given,
                        &|m| inc.at.diag(format!("including `{}`: {m}", inc.group)),
                        ", bind it here or expose it",
                    )?;
                    self.expand(&child, &child_iface, &child_values, out)?;
                }
                Item::Node(b) => out.push((self.node(def, b, &scope)?, Loc::of(&def.src, b))),
            }
        }
        Ok(())
    }

    fn node(&self, def: &GroupDef, b: &Block, scope: &Scope) -> Result<Instance, Diag> {
        let src = &def.src;
        let fields = Fields::of(src, b, &["name", "args", "env"], &[])?;
        // the label says what runs, `name` what the running node is called, its own name by default
        let reference = one_label(src, b)?;
        let node = refs::resolve(self.model, reference, scope.file_dir).map_err(|e| src.diag_at(b, e))?;
        crate::plan::runnable(&node).map_err(|e| src.diag_at(b, e))?;
        let name = match fields.attr("name") {
            Some(a) => eval::string(src, a, scope)?,
            None => node.name.clone(),
        };
        if name.is_empty() {
            return Err(src.diag_at(b, "a node name cannot be empty"));
        }
        let mut argv = node.run.clone();
        if let Some(a) = fields.attr("args") {
            match eval::value(src, a, scope)? {
                Value::String(s) => argv.extend(argv::split(&s).map_err(|e| src.diag_at(&a.value, format!("`args`: {e}")))?),
                Value::Array(items) => argv.extend(items.iter().map(param::text)),
                v => return Err(src.diag_at(&a.value, format!("`args` must be a string or a list, found {}", eval::type_name(&v)))),
            }
        }
        let mut env = BTreeMap::new();
        if let Some(a) = fields.attr("env") {
            match eval::value(src, a, scope)? {
                Value::Object(map) => {
                    for (k, v) in map {
                        if matches!(v, Value::Array(_) | Value::Object(_)) {
                            return Err(src.diag_at(&a.value, format!("`env` value `{k}` must be text, a number or a bool")));
                        }
                        env.insert(k, param::text(&v));
                    }
                }
                v => return Err(src.diag_at(&a.value, format!("`env` must be an object, found {}", eval::type_name(&v)))),
            }
        }
        Ok(Instance { name, cwd: node.cwd.clone(), argv, env, node })
    }
}

/// The same node as far as running it goes: what it runs, how, and where.
fn same(a: &Instance, b: &Instance) -> bool {
    a.argv == b.argv && a.env == b.env && a.cwd == b.cwd && a.type_ref() == b.type_ref()
}
