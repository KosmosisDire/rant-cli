//! Groups: one file each, a parameterized set of nodes and included groups. A group is
//! read once, its interface (own params plus the params its includes expose) is worked
//! out, and starting it expands it depth first into a flat plan.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hcl::Value;
use hcl_edit::structure::{Attribute, Block, Structure};
use hcl_edit::Span;

use crate::argv;
use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::model::{Model, Package};
use crate::paths;
use crate::plan::{Instance, Plan};
use crate::refs;
use crate::source::{no_labels, one_label, Fields, Source};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Int,
    Float,
    Bool,
}

impl ParamType {
    pub fn name(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Int => "int",
            ParamType::Float => "float",
            ParamType::Bool => "bool",
        }
    }
}

/// A place in a group file, kept so a later error can point back at it.
#[derive(Debug, Clone)]
struct Loc {
    file: PathBuf,
    line: u32,
    column: u32,
}

impl Loc {
    fn of(src: &Source, item: &dyn Span) -> Loc {
        let d = src.diag_at(item, "");
        Loc { file: d.file, line: d.line, column: d.column }
    }

    fn diag(&self, message: impl Into<String>) -> Diag {
        Diag::new(&self.file, self.line, self.column, message)
    }

    fn shown(&self) -> String {
        format!("{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: ParamType,
    pub options: Vec<Value>,
    pub default: Option<Value>,
    pub description: Option<String>,
    /// For an exposed param, the include it reaches: the group and the include's index.
    from: Option<(String, usize)>,
    at: Loc,
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

fn param_type(src: &Source, attr: &Attribute) -> Result<ParamType, Diag> {
    match eval::identifier(src, attr)? {
        "string" => Ok(ParamType::String),
        "int" => Ok(ParamType::Int),
        "float" => Ok(ParamType::Float),
        "bool" => Ok(ParamType::Bool),
        other => Err(src.diag_at(&attr.value, format!("unknown type `{other}`, expected string, int, float or bool"))),
    }
}

/// A value checked against a param's type, an int widened for a float, and against its
/// options. The error says what was wrong without a place, the caller adds one.
fn checked(p: &Param, v: Value) -> Result<Value, String> {
    let v = match (p.ty, v) {
        (ParamType::String, v @ Value::String(_)) => v,
        (ParamType::Bool, v @ Value::Bool(_)) => v,
        (ParamType::Int, Value::Number(n)) if n.as_i64().is_some() => Value::Number(n),
        (ParamType::Float, Value::Number(n)) => Value::from(n.as_f64().unwrap_or(0.0)),
        (ty, v) => return Err(format!("param `{}` is {}, not {}", p.name, ty.name(), eval::type_name(&v))),
    };
    if !p.options.is_empty() && !p.options.contains(&v) {
        let opts: Vec<String> = p.options.iter().map(text).collect();
        return Err(format!("param `{}` must be one of {}, not {}", p.name, opts.join(", "), text(&v)));
    }
    Ok(v)
}

/// A scalar as plain text, the form argv, env and root keys take.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A command line `key=value` value read as the param's type.
pub fn parse_value(p: &Param, raw: &str) -> Result<Value, String> {
    let v = match p.ty {
        ParamType::String => Value::from(raw),
        ParamType::Int => Value::from(raw.parse::<i64>().map_err(|_| format!("param `{}` takes an int, not `{raw}`", p.name))?),
        ParamType::Float => Value::from(raw.parse::<f64>().map_err(|_| format!("param `{}` takes a float, not `{raw}`", p.name))?),
        ParamType::Bool => match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(format!("param `{}` takes true or false, not `{raw}`", p.name)),
        },
    };
    checked(p, v)
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

    let mut params: Vec<Param> = Vec::new();
    for b in fields.blocks("param") {
        let name = one_label(&src, b)?.to_string();
        if params.iter().any(|p| p.name == name) {
            return Err(src.diag_at(b, format!("param `{name}` is declared twice")));
        }
        let pf = Fields::of(&src, b, &["type", "options", "default", "description"], &[])?;
        let ty = pf.attr("type").map(|a| param_type(&src, a)).transpose()?.unwrap_or(ParamType::String);
        let mut p = Param { name, ty, options: Vec::new(), default: None, description: None, from: None, at: Loc::of(&src, b) };
        if let Some(a) = pf.attr("options") {
            match eval::value(&src, a, &scope)? {
                Value::Array(opts) => {
                    for o in opts {
                        let o = checked(&Param { options: Vec::new(), ..p.clone() }, o).map_err(|e| src.diag_at(&a.value, e))?;
                        p.options.push(o);
                    }
                }
                v => return Err(src.diag_at(&a.value, format!("`options` must be a list, found {}", eval::type_name(&v)))),
            }
        }
        if let Some(a) = pf.attr("default") {
            p.default = Some(checked(&p, eval::value(&src, a, &scope)?).map_err(|e| src.diag_at(&a.value, format!("default: {e}")))?);
        }
        if let Some(a) = pf.attr("description") {
            p.description = Some(eval::string(&src, a, &scope)?);
        }
        params.push(p);
    }

    let mut items = Vec::new();
    for s in block.body.iter() {
        let Structure::Block(b) = s else { continue };
        match b.ident.as_str() {
            "include" => items.push(Item::Include(parse_include(&src, b, &scope)?)),
            "node" => {
                one_label(&src, b)?;
                let nf = Fields::of(&src, b, &["type", "name", "args", "env"], &[])?;
                if nf.attr("type").is_none() {
                    return Err(src.diag_at(b, "a node needs `type`, the node type it runs"));
                }
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
    fn bind(&self, iface: &[Param], given: BTreeMap<String, Value>, at: &dyn Fn(String) -> Diag, hint: &str) -> Result<BTreeMap<String, Value>, Diag> {
        let mut out = BTreeMap::new();
        for p in iface {
            let v = match given.get(&p.name) {
                Some(v) => checked(p, v.clone()).map_err(|e| at(e))?,
                None => match &p.default {
                    Some(d) => d.clone(),
                    None => return Err(at(format!("required param `{}` is not set{hint}", p.name))),
                },
            };
            out.insert(p.name.clone(), v);
        }
        Ok(out)
    }

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
        let values = self.bind(&iface, given, &|m| Diag::plain(format!("{m}{help}")), "")?;
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
                    let child_values = self.bind(
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
        let fields = Fields::of(src, b, &["type", "name", "args", "env"], &[])?;
        let ty_attr = fields.attr("type").expect("checked at parse");
        let reference = eval::string(src, ty_attr, scope)?;
        let node = refs::resolve(self.model, &reference, scope.file_dir).map_err(|e| src.diag_at(&ty_attr.value, e))?;
        let name = match fields.attr("name") {
            Some(a) => eval::string(src, a, scope)?,
            None => one_label(src, b)?.to_string(),
        };
        if name.is_empty() {
            return Err(src.diag_at(b, "a node name cannot be empty"));
        }
        let mut argv = node.run.clone();
        if let Some(a) = fields.attr("args") {
            match eval::value(src, a, scope)? {
                Value::String(s) => argv.extend(argv::split(&s).map_err(|e| src.diag_at(&a.value, format!("`args`: {e}")))?),
                Value::Array(items) => argv.extend(items.iter().map(text)),
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
                        env.insert(k, text(&v));
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
