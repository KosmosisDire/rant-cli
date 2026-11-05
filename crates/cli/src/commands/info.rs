//! `rant info`: one thing explained, a node running or not, an entity, a type, a package or a
//! group, and every one when a name means several.

use std::time::Duration;

use rant::{Entity, EntityKind, Schema};
use rant_config::model::{NodeType, Package};
use rant_config::{GroupInfo, Workspace, param};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::commands::kinds::Kind;
use crate::commands::run::snapshot;
use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::library;
use crate::mesh::{Client, FULL_SETTLE, Peer, kind_name, schema_text, standard_types, types_in_use};
use crate::nodes::managed_peers;
use crate::output::{Data, Style, Text, Yaml};
use crate::packages;
use crate::state::{Instance, RootKind, State};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a name, or a kind and a name such as `package cam`
    #[arg(value_name = "name", required = true)]
    pub words: Vec<String>,
}

/// One answer: its fields in order, and the schemas among them, which text prints as schema
/// text rather than as a value.
struct Answer {
    kind: Kind,
    fields: Map<String, Value>,
    schemas: Vec<(String, Option<Schema>)>,
    /// A standard type spelled out.
    whole: bool,
}

impl Answer {
    fn new(kind: Kind) -> Answer {
        Answer { kind, fields: Map::new(), schemas: Vec::new(), whole: false }
    }

    fn set(&mut self, key: &str, v: impl Into<Value>) {
        self.fields.insert(key.to_string(), v.into());
    }

    /// A schema field: plain text for a program, schema text for a person.
    fn schema(&mut self, key: &str, s: Option<Schema>) {
        self.set(key, schema_text(s.as_ref(), &Text::plain(), self.whole));
        self.schemas.push((key.to_string(), s));
    }
}

struct Answers(Vec<Answer>);

impl Serialize for Answers {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(|a| &a.fields))
    }
}

impl Data for Answers {
    fn text(&self, t: &Text) -> String {
        self.0.iter().map(|a| answer_text(a, t)).collect::<Vec<_>>().join("\n\n")
    }
}

/// An answer as YAML, keys lined up. A schema of one line sits after its key, a longer one
/// goes below it as a block.
fn answer_text(a: &Answer, t: &Text) -> String {
    let mut yaml = Yaml::new(t.colored(), false);
    let width = a.fields.keys().map(|k| k.chars().count()).max().unwrap_or(0);
    let mut lines = Vec::new();
    for (k, v) in &a.fields {
        let Some((_, schema)) = a.schemas.iter().find(|(key, _)| key == k) else {
            lines.push(yaml.entry(k, v, width));
            continue;
        };
        let text = schema_text(schema.as_ref(), t, a.whole);
        if !text.contains('\n') {
            lines.push(format!("{} {text}", yaml.key(k, width)));
            continue;
        }
        lines.push(format!("{} {}", yaml.key(k, width), t.paint(Style::Faint, "|")));
        lines.extend(text.lines().map(|l| if l.is_empty() { String::new() } else { format!("  {l}") }));
    }
    lines.join("\n")
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let (kind, name) = match args.words.as_slice() {
        [name] => (None, name),
        [k, name] => match Kind::named(k).filter(|k| k.explained()) {
            Some(k) => (Some(k), name),
            None => return Err(usage(format!("`{k}` is not a kind, use node, entity, topic, var, fn, task, type, package or group"))),
        },
        _ => return Err(usage("info takes a name, or a kind and a name such as `rant info package cam`")),
    };
    let st = snapshot(ctx)?;
    let mut answers = Vec::new();
    // a node is a running one or one that could start, so it looks in both places
    if kind.is_none_or(|k| !k.on_mesh() || k == Kind::Node) {
        workspace_answers(ctx, kind, name, &st, &mut answers)?;
    }
    // the answers' schemas live in the client's node, so it outlives their printing
    let client = match kind.is_none_or(Kind::on_mesh) {
        true => Some(Client::new(ctx.domain()?, Some(ctx.home()?))?),
        false => None,
    };
    if let Some(mesh) = &client {
        mesh_answers(mesh, kind, name, &st, &mut answers);
    }
    if answers.is_empty() {
        return Err(failed(match kind {
            Some(k) => format!("no {} named `{name}`, see `rant ls`", k.word()),
            None => format!("nothing named `{name}`, see `rant ls`"),
        }));
    }
    let mut kinds: Vec<&str> = Vec::new();
    for a in &answers {
        if !kinds.contains(&a.kind.word()) {
            kinds.push(a.kind.word());
        }
    }
    if kinds.len() > 1 {
        let article = |w: &str| if w.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
        let each: Vec<String> = kinds.iter().map(|k| format!("{} {k}", article(k))).collect();
        let (last, rest) = each.split_last().expect("more than one");
        ctx.out.note(&format!("`{name}` names {} and {last}, `rant info {} {name}` shows only one", rest.join(", "), kinds[0]));
    }
    ctx.show(&Answers(answers));
    Ok(())
}

// ---- the workspace ----

/// What the workspace has by that name: packages by name or folder, node types by reference or
/// bare name, and the group a reference finds.
fn workspace_answers(ctx: &mut Context, kind: Option<Kind>, name: &str, st: &State, out: &mut Vec<Answer>) -> Result<()> {
    if ctx.scope().is_none() {
        return Ok(());
    }
    let ws = ctx.require_scope()?;
    let want = |k: Kind| kind.is_none_or(|w| w == k);
    if want(Kind::Package) {
        let folder = ctx.cwd()?.join(name);
        for p in ws.model.packages.iter().filter(|p| p.name == name || (folder.is_dir() && rant_config::paths::same(&p.dir, &folder))) {
            out.push(package_answer(ctx, &ws, st, p)?);
        }
    }
    if want(Kind::Node) {
        for n in ws.model.node_types().filter(|n| n.reference() == name || n.name == name) {
            out.push(type_answer(ctx, &ws, st, n));
        }
    }
    if want(Kind::Group)
        && let Ok(g) = ws.group(name)
    {
        out.push(group_answer(ctx, &ws, st, &g));
    }
    Ok(())
}

fn running(st: &State, n: &NodeType) -> Vec<String> {
    st.instances.iter().filter(|i| i.node_type == n.type_ref()).map(|i| i.name.clone()).collect()
}

fn package_answer(ctx: &mut Context, ws: &Workspace, st: &State, p: &Package) -> Result<Answer> {
    let mut a = Answer::new(Kind::Package);
    a.set("package", p.name.clone());
    a.set("folder", ctx.shown(&p.dir));
    if let Some(d) = p.description() {
        a.set("description", d);
    }
    let home = ctx.home()?.to_path_buf();
    if let Some(rec) = p.installed.then(|| packages::record(&home, &p.name)).flatten() {
        if !rec.version.is_empty() {
            a.set("version", rec.version.clone());
        }
        a.set("source", if rec.repo.is_empty() { rant_config::paths::slash(&rec.path) } else { rec.repo.clone() });
        if !rec.extension.is_empty() {
            let profile = if rec.profile.is_empty() { String::new() } else { format!(", profile {}", rec.profile) };
            a.set("extension", format!("{} in {}{profile}", rec.extension, rant_config::paths::slash(&rec.editor)));
        }
    }
    a.set("kind", json!(p.kinds()));
    let uses = library::in_folder(&p.dir, &library::known(&home), false);
    let libraries: Vec<String> = uses.iter().map(|u| format!("{} {} through {} in {}", u.library, u.version.as_deref().unwrap_or("none"), u.how, ctx.shown(&u.file))).collect();
    a.set("libraries", json!(libraries));
    let (mut depends, mut build): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    // an installed package came built
    if !p.installed {
        let b = ws.build_plan(std::slice::from_ref(&p.name))?;
        depends.extend(b.edges.iter().filter(|e| e.from == p.name).map(|e| format!("{} ({})", e.to, e.source)));
        for step in b.steps.iter().filter(|s| s.package == p.name) {
            build.extend(step.configure.iter().map(|c| format!("{} (asks first)", rant_process::shown(c))));
            build.extend(step.commands.iter().map(|c| rant_process::shown(c)));
        }
    }
    a.set("depends", json!(depends));
    a.set("build", json!(build));
    a.set("nodes", json!(p.nodes.iter().map(NodeType::reference).collect::<Vec<_>>()));
    a.set("running", json!(p.nodes.iter().flat_map(|n| running(st, n)).collect::<Vec<_>>()));
    Ok(a)
}

fn type_answer(ctx: &Context, ws: &Workspace, st: &State, n: &NodeType) -> Answer {
    let mut a = Answer::new(Kind::Node);
    a.set("node", n.reference());
    a.set("kind", n.kind.name());
    if let Some(f) = &n.path {
        a.set("file", ctx.shown(f));
    }
    if let Some(s) = library::node_source(n) {
        a.set("source", ctx.shown(&s));
    }
    a.set("run", rant_process::shown(&n.run));
    a.set("cwd", ctx.shown(&n.cwd));
    let in_plan = |g: &str| ws.plan_group(g, &[]).is_ok_and(|p| p.plan.instances.iter().any(|i| i.type_ref() == n.type_ref()));
    let groups: Vec<&str> = ws.model.groups.iter().map(|g| g.name.as_str()).filter(|g| in_plan(g)).collect();
    a.set("groups", json!(groups));
    a.set("running", json!(running(st, n)));
    a
}

fn group_answer(ctx: &Context, ws: &Workspace, st: &State, g: &GroupInfo) -> Answer {
    let mut a = Answer::new(Kind::Group);
    a.set("group", g.file.name.clone());
    a.set("file", ctx.shown(&g.file.path));
    if let Some(d) = g.description.as_deref().filter(|d| !d.is_empty()) {
        a.set("description", d);
    }
    let mut params = Map::new();
    for p in &g.params {
        let mut about = format!("{}{}", p.ty.name(), p.default.as_ref().map_or_else(|| ", required".to_string(), |d| format!(", default {}", param::text(d))));
        if !p.options.is_empty() {
            about += &format!(", one of {}", p.options.iter().map(param::text).collect::<Vec<_>>().join(", "));
        }
        params.insert(p.name.clone(), Value::String(about));
    }
    a.set("params", params);
    match ws.plan_group(&g.file.name, &[]) {
        Ok(plan) => a.set("nodes", plan.plan.instances.iter().map(|i| (i.name.clone(), Value::String(i.type_ref()))).collect::<Map<_, _>>()),
        Err(_) => a.set("nodes", "set its required params to see them"),
    }
    let runs = st.roots.iter().filter(|r| r.kind == RootKind::Group && r.name == g.file.name);
    a.set("running", json!(runs.map(|r| r.key()["group ".len()..].to_string()).collect::<Vec<_>>()));
    a
}

// ---- the mesh ----

/// What a node does with an entity, in the words a reader expects for its kind.
fn roles(e: &Entity) -> Vec<&'static str> {
    let (topic, var) = (e.kind == EntityKind::Topic, e.kind == EntityKind::Variable);
    let mut out = Vec::new();
    if e.provides {
        out.push(if topic { "publishes" } else if var { "owns" } else { "serves" });
    }
    if e.consumes {
        out.push(if topic { "subscribes" } else if var { "reads" } else { "calls" });
    }
    out
}

fn mesh_answers(mesh: &Client, kind: Option<Kind>, name: &str, st: &State, out: &mut Vec<Answer>) {
    mesh.settle(FULL_SETTLE);
    if kind.is_none_or(|k| k == Kind::Node) {
        for (inst, peer) in find_nodes(mesh, st, name) {
            out.push(node_answer(mesh, inst, peer.as_ref()));
        }
    }
    for e in mesh.entities().into_iter().filter(|e| e.name == name && kind.is_none_or(|k| k.takes(e.kind))) {
        out.push(entity_answer(mesh, &e));
    }
    if kind.is_none_or(|k| k == Kind::Type) {
        out.extend(type_of(mesh, name));
    }
}

/// Every node by that name: rant's instances first, each with its mesh node when it is on the
/// mesh, then mesh nodes rant did not start.
fn find_nodes<'a>(mesh: &Client, st: &'a State, name: &str) -> Vec<(Option<&'a Instance>, Option<Peer>)> {
    let managed = managed_peers(mesh, st);
    let peers = mesh.peers();
    let instance_of = |peer: u32| managed.get(&peer).and_then(|n| st.instance(n));
    let mut out = Vec::new();
    for inst in st.instances.iter().filter(|i| i.name == name) {
        let peer = peers.iter().rev().find(|p| instance_of(p.id).is_some_and(|i| std::ptr::eq(i, inst))).cloned();
        out.push((Some(inst), peer));
    }
    for p in peers.iter().filter(|p| p.name == name) {
        let inst = instance_of(p.id);
        // shown with its instance
        if inst.is_none_or(|i| i.name != name) {
            out.push((inst, Some(p.clone())));
        }
    }
    out
}

fn node_answer(mesh: &Client, inst: Option<&Instance>, peer: Option<&Peer>) -> Answer {
    let mut a = Answer::new(Kind::Node);
    a.set("node", inst.map_or_else(|| peer.map(|p| p.name.clone()).unwrap_or_default(), |i| i.name.clone()));
    if let Some(i) = inst {
        a.set("from", i.node_type.clone());
        a.set("run", json!(i.argv));
        a.set("log", i.log.clone());
        a.set("roots", json!(i.roots));
    }
    let Some(peer) = peer else {
        a.set("pid", inst.map_or(0, |i| i.pid));
        a.set("mesh", "not on the mesh");
        return a;
    };
    a.set("address", peer.address.clone());
    if let Some(pid) = mesh.pids(&[peer.id], Duration::from_millis(500)).get(&peer.id) {
        a.set("pid", *pid);
    }
    for e in mesh.entities_of(peer.id) {
        for r in roles(&e) {
            let list = a.fields.entry(r).or_insert_with(|| json!([]));
            if let Value::Array(names) = list {
                names.push(Value::String(e.name.clone()));
            }
        }
    }
    a
}

fn entity_answer(mesh: &Client, e: &Entity) -> Answer {
    let mut a = Answer::new(Kind::Entity);
    a.set(kind_name(e.kind), e.name.clone());
    if matches!(e.kind, EntityKind::Function | EntityKind::Task) {
        a.schema("request", e.schema.clone());
        a.schema("response", e.response_schema.clone());
        if e.kind == EntityKind::Task {
            a.schema("progress", e.progress_schema.clone());
        }
    } else {
        a.schema("type", e.schema.clone());
    }
    let (mut providers, mut consumers) = (Vec::new(), Vec::new());
    for p in mesh.peers() {
        for pe in mesh.entities_of(p.id).iter().filter(|pe| pe.kind == e.kind && pe.name == e.name) {
            if pe.provides {
                providers.push(p.name.clone());
            }
            if pe.consumes {
                consumers.push(p.name.clone());
            }
        }
    }
    let (topic, var) = (e.kind == EntityKind::Topic, e.kind == EntityKind::Variable);
    a.set(if topic { "publishers" } else if var { "owner" } else { "providers" }, json!(providers));
    a.set(if topic { "subscribers" } else if var { "readers" } else { "callers" }, json!(consumers));
    if var {
        a.set("value", mesh.read_variable(&e.name, Duration::from_secs(2)).unwrap_or(Value::Null));
    }
    a
}

/// A type by name: one the mesh uses, with what uses it, else a standard type, known without
/// the mesh.
fn type_of(mesh: &Client, name: &str) -> Option<Answer> {
    let used = types_in_use(&mesh.entities()).remove(name);
    let schema = match &used {
        Some(u) => u.schema.clone(),
        None if standard_types().iter().any(|t| t == name) => mesh.node().schema(name).ok()?,
        None => return None,
    };
    let mut a = Answer::new(Kind::Type);
    a.whole = true;
    a.set("type", name);
    a.schema("definition", Some(schema));
    a.set("used by", json!(used.map(|u| u.entities).unwrap_or_default()));
    Some(a)
}
