//! `rant ls`: what runs, what could start and what the mesh offers, every entity on the mesh
//! when no kind is named.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rant::Entity;
use rant_config::model::NodeType;
use rant_config::param;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::commands::enable::{scope_at, service_key};
use crate::commands::kinds::Kind;
use crate::commands::run::snapshot;
use crate::context::Context;
use crate::files::same_path;
use crate::failure::{Result, usage};
use crate::library::{self, Library};
use crate::mesh::{Client, FULL_SETTLE, kind_name, types_in_use};
use crate::names::matches;
use crate::nodes::managed_peers;
use crate::output::{Data, Format, Style, Table, Text, bytes, columns, iso_ms};
use crate::packages;
use crate::packages::sources::Sources;
use crate::recording;
use crate::services::{self, When, entry};
use crate::state::{self, Instance, RootKind, State, Store};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a kind and a pattern, the names holding it or matching it with * and ?
    #[arg(value_name = "kind")]
    pub words: Vec<String>,
    /// with nodes: only what this workspace declares and started, without joining the mesh
    #[arg(long)]
    pub no_mesh: bool,
    /// with packages or libraries: what `rant install` or `rant add` can take, from config.rant and the package databases
    #[arg(long)]
    pub available: bool,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let kind = match args.words.first() {
        Some(w) => Kind::named(w).ok_or_else(|| usage(format!("`{w}` is not a kind ls lists, see `rant ls -h`")))?,
        None => Kind::Entity,
    };
    if args.words.len() > 2 {
        return Err(usage("ls takes a kind and one pattern at most, such as `rant ls types cam`"));
    }
    let pattern = args.words.get(1).map_or("", String::as_str);
    if args.available && !matches!(kind, Kind::Package | Kind::Library) {
        return Err(usage("--available goes with `rant ls packages` or `rant ls libraries`"));
    }
    if args.no_mesh && kind != Kind::Node {
        return Err(usage("--no-mesh goes with `rant ls nodes`"));
    }
    if kind == Kind::Service {
        return list_services(ctx, pattern);
    }
    if kind == Kind::Source {
        if !pattern.is_empty() {
            return Err(usage("ls sources takes no pattern"));
        }
        return list_sources(ctx);
    }
    if args.available {
        return list_available(ctx, kind, pattern);
    }
    match kind {
        Kind::Node => {
            let d = list_nodes(ctx, pattern, !args.no_mesh)?;
            ctx.show(&d);
        }
        Kind::Type => {
            let d = list_types(ctx, pattern)?;
            ctx.show(&d);
        }
        Kind::Package => {
            let d = list_packages(ctx, pattern)?;
            ctx.show(&d);
        }
        Kind::Group => {
            let d = list_groups(ctx, pattern)?;
            ctx.show(&d);
        }
        Kind::Library => list_libraries(ctx, pattern)?,
        Kind::Recording => {
            let d = list_recordings(ctx, pattern)?;
            ctx.show(&d);
        }
        _ => {
            let d = list_entities(ctx, kind, pattern)?;
            ctx.show(&d);
        }
    }
    Ok(())
}

/// A titled list of lines, with a note when there are none.
fn section(t: &Text, title: &str, lines: Vec<String>) -> String {
    let body = if lines.is_empty() { vec![t.paint(Style::Faint, "  (none)")] } else { lines };
    std::iter::once(t.paint(Style::Bold, title)).chain(body).collect::<Vec<_>>().join("\n")
}

fn joined(v: &[impl AsRef<str>], sep: &str) -> String {
    v.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(sep)
}

fn mesh(ctx: &mut Context) -> Result<Client> {
    let mesh = Client::new(ctx.domain()?, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    Ok(mesh)
}

// ---- nodes ----

/// One node as ls lists it. on_mesh is unknown without the mesh, or for a program declared to
/// never join, and false for one still joining or not using Rant.
#[derive(Debug, Clone, Serialize)]
struct Row {
    name: String,
    /// The node type of one rant started.
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    node_type: String,
    /// The domain of one rant started.
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<u16>,
    /// The group whose own block made one rant started.
    #[serde(skip_serializing_if = "String::is_empty")]
    group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    on_mesh: Option<bool>,
}

impl Row {
    fn of(i: &Instance, joined: Option<bool>) -> Row {
        // the RANT_DOMAIN rant gave it, else Rant's 0
        let domain = i.env.get("RANT_DOMAIN").and_then(|d| d.parse().ok()).unwrap_or(0);
        Row { name: i.name.clone(), node_type: i.node_type.clone(), domain: Some(domain), group: i.group.clone(), on_mesh: joined }
    }
}

/// A run of a group with its nodes. A group the run includes is part of it, its nodes there
/// with their own `group`.
#[derive(Debug, Serialize)]
struct Run {
    group: String,
    nodes: Vec<Row>,
}

/// The running nodes by group as text shows them: each group root with its nodes, then the
/// groups it includes one step in.
struct Shown {
    header: String,
    nodes: Vec<Row>,
    inside: bool,
}

#[derive(Debug, Serialize)]
struct Idle {
    name: String,
    kind: &'static str,
    path: Option<String>,
    #[serde(skip)]
    file: Option<PathBuf>,
    #[serde(skip)]
    run: Vec<String>,
}

#[derive(Serialize)]
struct Nodes {
    /// The mesh this listing reads, the workspace's, so a tool can join the same one.
    domain: u16,
    groups: Vec<Run>,
    /// Every node outside a group, started by rant or not.
    nodes: Vec<Row>,
    not_running: Vec<Idle>,
    #[serde(skip)]
    shown: Vec<Shown>,
}

fn list_nodes(ctx: &mut Context, pattern: &str, on_mesh: bool) -> Result<Nodes> {
    let st = snapshot(ctx)?;
    let mesh = if on_mesh { Some(mesh(ctx)?) } else { None };
    let managed = mesh.as_ref().map(|m| managed_peers(m, &st)).unwrap_or_default();
    let joined: BTreeSet<&String> = managed.values().collect();
    let row = |i: &Instance| Row::of(i, (mesh.is_some() && i.expect_mesh).then(|| joined.contains(&i.name)));

    let mut shown: Vec<Shown> = Vec::new();
    for r in st.roots.iter().filter(|r| r.kind == RootKind::Group) {
        // the params show only when the group runs more than once
        let runs = st.roots.iter().filter(|o| o.kind == RootKind::Group && o.name == r.name).count();
        let key = r.key();
        let mut own = Shown { header: if runs > 1 { key["group ".len()..].to_string() } else { r.name.clone() }, nodes: Vec::new(), inside: false };
        // the nodes of groups it includes go under those groups, in the order they first come
        let mut inside: Vec<Shown> = Vec::new();
        for i in st.instances.iter().filter(|i| i.roots.contains(&key) && matches(pattern, &i.name)) {
            if i.group.is_empty() || i.group == r.name {
                own.nodes.push(row(i));
            } else if let Some(g) = inside.iter_mut().find(|g| g.header == i.group) {
                g.nodes.push(row(i));
            } else {
                inside.push(Shown { header: i.group.clone(), nodes: vec![row(i)], inside: true });
            }
        }
        if own.nodes.is_empty() && inside.is_empty() {
            continue;
        }
        shown.push(own);
        shown.extend(inside);
    }
    let in_group = |i: &Instance| i.roots.iter().any(|k| k.starts_with("group "));
    let mut loose: Vec<Row> = st.instances.iter().filter(|i| !in_group(i) && matches(pattern, &i.name)).map(row).collect();
    if let Some(m) = &mesh {
        let others = m.peers().into_iter().filter(|p| !managed.contains_key(&p.id) && matches(pattern, &p.name));
        loose.extend(others.map(|p| Row { name: p.name, node_type: String::new(), domain: None, group: String::new(), on_mesh: Some(true) }));
    }

    let mut groups: Vec<Run> = Vec::new();
    for s in &shown {
        match groups.last_mut() {
            Some(run) if s.inside => run.nodes.extend(s.nodes.iter().cloned()),
            _ => groups.push(Run { group: s.header.clone(), nodes: s.nodes.clone() }),
        }
    }
    let not_running = not_running(ctx, &st, pattern)?;
    Ok(Nodes { domain: ctx.domain()?, groups, nodes: loose, not_running, shown })
}

/// The nodes this scope could start that are not running.
fn not_running(ctx: &mut Context, st: &State, pattern: &str) -> Result<Vec<Idle>> {
    if ctx.scope().is_none() {
        return Ok(Vec::new());
    }
    let ws = ctx.require_scope()?;
    let runs = |n: &NodeType| st.instances.iter().any(|i| i.node_type == n.type_ref());
    let idle = ws.model.node_types().filter(|n| !runs(n) && matches(pattern, &n.reference()));
    Ok(idle
        .map(|n| Idle {
            name: n.reference(),
            kind: n.kind.name(),
            path: n.path.as_deref().map(rant_config::paths::slash),
            file: n.path.clone(),
            run: n.run.clone(),
        })
        .collect())
}

impl Data for Nodes {
    fn text(&self, t: &Text) -> String {
        // every list of nodes on one grid, those outside a group under a header of their own
        // when there are groups, so the columns line up all the way down
        let cell = |r: &Row| match r.on_mesh {
            Some(on) if t.colored() => vec![format!("{} {}", t.paint(if on { Style::Green } else { Style::Amber }, "\u{25cf}"), r.name)],
            _ => vec![r.name.clone()],
        };
        let grouped = !self.shown.is_empty();
        let mut lists: Vec<(String, bool, Vec<Vec<String>>)> = self.shown.iter().map(|s| (s.header.clone(), s.inside, s.nodes.iter().map(cell).collect())).collect();
        if !self.nodes.is_empty() {
            lists.push(("other".to_string(), false, self.nodes.iter().map(cell).collect()));
        }
        let cells: Vec<Vec<Vec<String>>> = lists.iter().map(|(_, _, c)| c.clone()).collect();
        let laid = columns(&cells, t.width(), if grouped { "    " } else { "  " });
        let mut lines = Vec::new();
        for ((header, inside, _), rows) in lists.iter().zip(laid) {
            // a group a run includes sits one step in, under that run
            let step = if *inside { "  " } else { "" };
            if grouped {
                lines.push(format!("  {step}{}", t.paint(Style::Faint, &format!("{header}:"))));
            }
            lines.extend(rows.into_iter().map(|l| format!("{step}{l}")));
        }
        let mut idle = Table::default();
        for n in &self.not_running {
            let place = n.file.as_deref().map_or_else(|| rant_process::shown(&n.run), |f| t.path(f));
            idle.row(vec![n.name.clone(), t.paint(Style::Dim, n.kind), t.paint(Style::Dim, &place)]);
        }
        format!("{}\n\n{}", section(t, "NODES", lines), section(t, "NOT RUNNING", idle.lines("  ")))
    }
}

// ---- entities and types ----

#[derive(Serialize)]
struct Named {
    name: String,
    kind: &'static str,
}

#[derive(Serialize)]
struct Entities {
    entities: Vec<Named>,
}

/// The explorer's color for each entity kind, as its mesh view draws it.
fn kind_style(kind: &str) -> Style {
    match kind {
        "fn" => Style::Accent,
        "var" => Style::Amber,
        "task" => Style::Green,
        _ => Style::Dim,
    }
}

impl Data for Entities {
    fn text(&self, t: &Text) -> String {
        let cells: Vec<Vec<String>> = self.entities.iter().map(|e| vec![e.name.clone(), t.paint(kind_style(e.kind), e.kind)]).collect();
        section(t, "ENTITIES", columns(&[cells], t.width(), "  ").concat())
    }
}

fn list_entities(ctx: &mut Context, kind: Kind, pattern: &str) -> Result<Entities> {
    let mesh = mesh(ctx)?;
    let wanted = |e: &Entity| matches(pattern, &e.name) && kind.takes(e.kind);
    let entities = mesh.entities().into_iter().filter(wanted).map(|e| Named { kind: kind_name(e.kind), name: e.name }).collect();
    Ok(Entities { entities })
}

#[derive(Serialize)]
struct TypeRow {
    #[serde(rename = "type")]
    name: String,
    used_by: Vec<String>,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Types(Vec<TypeRow>);

impl Data for Types {
    fn text(&self, t: &Text) -> String {
        let mut table = Table::default();
        for r in &self.0 {
            table.row(vec![r.name.clone(), t.paint(Style::Dim, &joined(&r.used_by, ", "))]);
        }
        section(t, "TYPES", table.lines("  "))
    }
}

/// The named types the mesh's entities use, each with the entities that use it.
fn list_types(ctx: &mut Context, pattern: &str) -> Result<Types> {
    let mesh = mesh(ctx)?;
    let used = types_in_use(&mesh.entities()).into_iter().filter(|(name, _)| matches(pattern, name));
    Ok(Types(used.map(|(name, u)| TypeRow { name, used_by: u.entities }).collect()))
}

// ---- recordings ----

#[derive(Serialize)]
struct RecordingRow {
    name: String,
    file: String,
    bytes: u64,
    modified: String,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Recordings(Vec<RecordingRow>);

impl Data for Recordings {
    fn text(&self, t: &Text) -> String {
        let mut table = Table::default();
        for r in &self.0 {
            table.row(vec![r.name.clone(), t.paint(Style::Dim, &bytes(r.bytes)), t.paint(Style::Dim, &r.modified)]);
        }
        section(t, "RECORDINGS", table.lines("  "))
    }
}

/// The recordings in the recordings folder, by name, which is their start time unless named.
fn list_recordings(ctx: &mut Context, pattern: &str) -> Result<Recordings> {
    let mut rows = Vec::new();
    for path in recording::listed(ctx)? {
        let name = recording::name_of(&path);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !matches(pattern, &name) {
            continue;
        }
        let since = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_micros() as u64);
        rows.push(RecordingRow { name, file: ctx.shown(&path), bytes: meta.len(), modified: format!("{}Z", &iso_ms(since)[..19]) });
    }
    Ok(Recordings(rows))
}

// ---- the workspace ----

#[derive(Serialize)]
struct PackageRow {
    name: String,
    kinds: Vec<&'static str>,
    folder: String,
    files: Vec<String>,
    libraries: Map<String, Value>,
    installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip)]
    dir: PathBuf,
    #[serde(skip)]
    linked: bool,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Packages(Vec<PackageRow>);

impl Data for Packages {
    fn text(&self, t: &Text) -> String {
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut table = Table::default();
        for p in &self.0 {
            let uses = if p.installed {
                let state = match (&p.version, p.linked) {
                    (_, true) => "linked".to_string(),
                    (Some(v), _) => format!("installed {v}"),
                    _ => "installed".to_string(),
                };
                dim(&state)
            } else if p.libraries.is_empty() {
                dim("no library")
            } else {
                p.libraries.iter().map(|(name, v)| format!("{name} {}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join(", ")
            };
            table.row(vec![p.name.clone(), dim(&joined(&p.kinds, ", ")), uses, dim(&t.path(&p.dir))]);
        }
        section(t, "PACKAGES", table.lines("  "))
    }
}

/// The version of each library a package uses, the installed one for Python.
fn library_versions(dir: &Path, libs: &[Library]) -> Map<String, Value> {
    let mut out = Map::new();
    for u in library::in_folder(dir, libs, false) {
        let Some(v) = u.version else { continue };
        if !out.contains_key(&u.library) || u.how != "pyproject" {
            out.insert(u.library, Value::String(v));
        }
    }
    out
}

/// The packages of the workspace and the installed ones, running or not.
fn list_packages(ctx: &mut Context, pattern: &str) -> Result<Packages> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let libs = library::known(&home);
    let mut rows = Vec::new();
    for p in ws.model.packages.iter().filter(|p| matches(pattern, &p.name)) {
        let rec = if p.installed { packages::record(&home, &p.name) } else { None };
        rows.push(PackageRow {
            name: p.name.clone(),
            kinds: p.kinds(),
            folder: rant_config::paths::slash(&p.dir),
            files: p.files().iter().map(|f| rant_config::paths::slash(f)).collect(),
            libraries: library_versions(&p.dir, &libs),
            installed: p.installed,
            version: rec.as_ref().map(|r| r.version.clone()).filter(|v| !v.is_empty()),
            source: rec.as_ref().map(|r| if r.repo.is_empty() { rant_config::paths::slash(&r.path) } else { r.repo.clone() }),
            dir: p.dir.clone(),
            linked: p.installed && packages::is_link(&packages::dir(&home).join(&p.name)),
        });
    }
    Ok(Packages(rows))
}

#[derive(Serialize)]
struct GroupRow {
    name: String,
    file: String,
    description: String,
    params: String,
    includes: Vec<String>,
    /// Its own nodes as the defaults name them, none while a param has no default. Only for a
    /// program, since planning each group costs.
    #[serde(skip_serializing_if = "Option::is_none")]
    nodes: Option<Vec<GroupNode>>,
}

#[derive(Serialize)]
struct GroupNode {
    name: String,
    #[serde(rename = "type")]
    node_type: String,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Groups(Vec<GroupRow>);

impl Data for Groups {
    fn text(&self, t: &Text) -> String {
        let mut table = Table::default();
        for g in &self.0 {
            table.row(vec![g.name.clone(), t.paint(Style::Dim, &g.params), g.description.clone()]);
        }
        section(t, "GROUPS", table.lines("  "))
    }
}

fn list_groups(ctx: &mut Context, pattern: &str) -> Result<Groups> {
    let ws = ctx.require_scope()?;
    let program = ctx.out.format() != Format::Text;
    let mut rows = Vec::new();
    for file in ws.model.groups.iter().filter(|g| matches(pattern, &g.name)) {
        let info = match ws.group(&file.name) {
            Ok(info) => info,
            Err(d) => {
                ctx.out.warn(&d.to_string());
                continue;
            }
        };
        // a group's params in short: `speed=1`, or `target=?` for a required one
        let params: Vec<String> = info.params.iter().map(|p| format!("{}={}", p.name, p.default.as_ref().map_or_else(|| "?".to_string(), param::text))).collect();
        let nodes = program.then(|| {
            let plan = ws.plan_group(&file.name, &[]).map(|g| g.plan.instances).unwrap_or_default();
            let own = plan.into_iter().filter(|i| i.group.as_deref() == Some(file.name.as_str()));
            own.map(|i| GroupNode { node_type: i.type_ref(), name: i.name }).collect()
        });
        rows.push(GroupRow {
            name: file.name.clone(),
            file: rant_config::paths::slash(&file.path),
            description: info.description.unwrap_or_default(),
            params: params.join(" "),
            includes: info.includes,
            nodes,
        });
    }
    Ok(Groups(rows))
}

#[derive(Serialize)]
struct LibraryRow {
    library: String,
    folder: String,
    kind: &'static str,
    how: &'static str,
    version: Option<String>,
    file: String,
    #[serde(skip)]
    use_: library::Use,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Libraries(Vec<LibraryRow>);

impl Data for Libraries {
    fn text(&self, t: &Text) -> String {
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut table = Table::default();
        for r in &self.0 {
            let u = &r.use_;
            let place = if u.how == "venv" && !u.venv_exists { "no venv yet".to_string() } else { format!("{} {}", u.how, t.path(&u.file)) };
            table.row(vec![t.path(&u.dir), u.library.clone(), u.kind.name().to_string(), u.version.clone().unwrap_or_else(|| dim("none")), dim(&place)]);
        }
        section(t, "LIBRARIES", table.lines("  "))
    }
}

/// Where rant install looks, in order: the paths and repos config.rant lists, the official
/// database, then the databases config.rant adds.
#[derive(Serialize)]
#[serde(transparent)]
struct SourceList(Vec<Value>);

impl Data for SourceList {
    fn text(&self, t: &Text) -> String {
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut table = Table::default();
        for r in &self.0 {
            let (place, what) = ["path", "repo", "database"].iter().find_map(|k| Some((r[*k].as_str()?, *k))).unwrap_or_default();
            let official = if r["official"] == Value::Bool(true) { dim("official") } else { String::new() };
            table.row(vec![place.to_string(), dim(what), official]);
        }
        section(t, "SOURCES", table.lines("  "))
    }
}

fn list_sources(ctx: &mut Context) -> Result<()> {
    let home = ctx.home()?.to_path_buf();
    let sources = Sources::new(&home)?;
    let local = sources.local();
    let mut rows: Vec<Value> = local.paths.iter().map(|p| json!({"path": rant_config::paths::slash(p)})).collect();
    rows.extend(local.repos.iter().map(|r| json!({"repo": r})));
    rows.extend(sources.databases().iter().map(|d| json!({"database": d, "official": d == packages::OFFICIAL})));
    let none = local.paths.is_empty() && local.repos.is_empty() && local.databases.is_empty();
    ctx.show(&SourceList(rows));
    if none && ctx.out.format() == Format::Text {
        ctx.out.note(&format!("list your own in {}", rant_config::paths::slash(&rant_config::home::file(&home))));
    }
    Ok(())
}

/// What `rant install` or `rant add` can take, from config.rant and the package databases.
#[derive(Serialize)]
#[serde(transparent)]
struct Available {
    rows: Vec<Value>,
    #[serde(skip)]
    lines: Vec<[String; 4]>,
}

impl Data for Available {
    fn text(&self, t: &Text) -> String {
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut table = Table::default();
        for [name, from, state, description] in &self.lines {
            table.row(vec![name.clone(), dim(from), dim(state), description.clone()]);
        }
        section(t, "AVAILABLE", table.lines("  "))
    }
}

fn list_available(ctx: &mut Context, kind: Kind, pattern: &str) -> Result<()> {
    let mut sources = Sources::new(ctx.home()?)?;
    let offers = sources.offers(pattern)?;
    sources.problems().iter().for_each(|p| ctx.out.warn(p));
    let mut out = Available { rows: Vec::new(), lines: Vec::new() };
    for o in offers.into_iter().filter(|o| o.source.library.is_some() == (kind == Kind::Library)) {
        let s = &o.source;
        let from = if s.repo.is_empty() { rant_config::paths::slash(&s.path) } else { s.repo.clone() };
        if let Some(b) = &s.library {
            let languages: Vec<&str> = [("cmake", &b.cmake), ("python", &b.python), ("csharp", &b.csharp), ("rust", &b.rust), ("plcnext", &b.plcnext)].into_iter().filter(|(_, n)| n.is_some()).map(|(k, _)| k).collect();
            out.rows.push(json!({"name": s.name, "repo": s.repo, "description": o.description, "languages": languages}));
            out.lines.push([s.name.clone(), from, languages.join(" "), o.description.clone()]);
            continue;
        }
        let installed = match &o.record {
            None => Value::Bool(false),
            Some(r) if r.version.is_empty() => Value::Bool(true),
            Some(r) => json!(r.version),
        };
        let state = match &installed {
            Value::String(v) => format!("installed {v}"),
            Value::Bool(true) => "installed".into(),
            _ => String::new(),
        };
        let mut row = json!({"name": s.name, "description": o.description, "installed": installed});
        row[if s.repo.is_empty() { "path" } else { "repo" }] = json!(from);
        out.rows.push(row);
        out.lines.push([s.name.clone(), from, state, o.description.clone()]);
    }
    ctx.show(&out);
    Ok(())
}

/// Every use of a library the sources name under the workspace, else under here.
fn list_libraries(ctx: &mut Context, pattern: &str) -> Result<()> {
    let start = match ctx.workspace() {
        Some(s) => s.root.clone(),
        None => ctx.cwd()?.to_path_buf(),
    };
    let libs: Vec<Library> = library::known(ctx.home()?).into_iter().filter(|l| matches(pattern, &l.name)).collect();
    let uses = library::under(&start, &libs);
    if uses.is_empty() && ctx.out.format() == Format::Text {
        ctx.out.note("nothing here uses a library yet, `rant add` adds Rant to the package here");
        return Ok(());
    }
    let row = |u: library::Use| LibraryRow {
        library: u.library.clone(),
        folder: rant_config::paths::slash(&u.dir),
        kind: u.kind.name(),
        how: u.how,
        version: u.version.clone(),
        file: rant_config::paths::slash(&u.file),
        use_: u,
    };
    ctx.show(&Libraries(uses.into_iter().map(row).collect()));
    Ok(())
}

#[derive(Serialize)]
struct ServiceNode {
    name: String,
    running: bool,
    restart: &'static str,
    restarts: u32,
}

/// A service and where it stands: its nodes now, found by planning it as rant run would, and
/// the restarts the service process made.
#[derive(Serialize)]
struct ServiceRow {
    service: String,
    group: String,
    params: Vec<String>,
    folder: String,
    start: Option<&'static str>,
    state: String,
    restarts: u32,
    nodes: Vec<ServiceNode>,
}

/// A line of the table: a service, or none where a folder holds none.
struct ServiceLine {
    service: Option<String>,
    at: String,
    state: String,
    restarts: u32,
    folder: PathBuf,
}

#[derive(Serialize)]
#[serde(transparent)]
struct ServiceList {
    rows: Vec<ServiceRow>,
    #[serde(skip)]
    lines: Vec<ServiceLine>,
}

impl Data for ServiceList {
    fn text(&self, t: &Text) -> String {
        let dim = |s: &str| t.paint(Style::Dim, s);
        let mut table = Table::default();
        for l in &self.lines {
            let restarts = match l.restarts {
                0 => String::new(),
                1 => "1 restart".into(),
                n => format!("{n} restarts"),
            };
            let service = l.service.clone().unwrap_or_else(|| t.paint(Style::Faint, "(none)"));
            table.row(vec![service, dim(&l.at), l.state.clone(), dim(&restarts), dim(&t.path(&l.folder))]);
        }
        section(t, "SERVICES", table.lines("  "))
    }
}

/// The nodes of a scope's state that a root key holds, the state pruned first.
fn standing(home: &Path, data: &Path, key: &str) -> Result<(String, u32, Vec<ServiceNode>)> {
    let mut store = Store::open(data, true)?;
    state::prune(store.state(), |s| services::running_in(home, s));
    store.save()?;
    let (mut up, mut waiting, mut restarts, mut nodes) = (0, 0, 0, Vec::new());
    for i in store.state().instances.iter().filter(|i| i.roots.iter().any(|r| r == key)) {
        let running = rant_process::alive(&i.tracking());
        up += usize::from(running);
        waiting += usize::from(!running && i.restart != rant_config::plan::Restart::Never);
        restarts += i.restarts;
        nodes.push(ServiceNode { name: i.name.clone(), running, restart: i.restart.as_str(), restarts: i.restarts });
    }
    let all = nodes.len();
    let mut state = if all == 0 { "stopped".to_string() } else if up == all { "running".to_string() } else { format!("{up} of {all} running") };
    if waiting > 0 {
        state += &format!(", {waiting} restarting");
    }
    Ok((state, restarts, nodes))
}

/// The services of each enabled folder, then of this scope while it is not enabled, so what
/// rant enable would start shows.
fn list_services(ctx: &mut Context, pattern: &str) -> Result<()> {
    let home = ctx.home()?.to_path_buf();
    let list = services::enabled(&home)?;
    let mut folders: Vec<(PathBuf, Option<When>)> = list.iter().map(|e| (e.folder.clone(), Some(e.start))).collect();
    if let Some(here) = ctx.scope().map(|s| s.root.clone())
        && !list.iter().any(|e| same_path(&e.folder, &here))
    {
        folders.push((here, None));
    }
    let mut out = ServiceList { rows: Vec::new(), lines: Vec::new() };
    for (folder, start) in folders {
        let at = start.map_or_else(|| "not enabled".to_string(), |w| format!("at {}", w.word()));
        let none = |state: String| ServiceLine { service: None, at: at.clone(), state, restarts: 0, folder: folder.clone() };
        let ws = match scope_at(&home, &folder) {
            Ok(ws) => ws,
            Err(why) => {
                out.lines.push(none(why));
                continue;
            }
        };
        let checks = ws.services();
        if checks.is_empty() && pattern.is_empty() {
            out.lines.push(none(String::new()));
        }
        for check in checks.iter().filter(|c| matches(pattern, &c.service.group)) {
            let (state, restarts, nodes) = match service_key(check) {
                Ok(key) => standing(&home, &ws.model.settings.data, &key)?,
                Err(why) => (why, 0, Vec::new()),
            };
            let (group, params) = match &check.checked {
                Ok(c) => (c.full.clone(), c.words.clone()),
                Err(_) => (check.service.group.clone(), Vec::new()),
            };
            let service = check.shown();
            out.lines.push(ServiceLine { service: Some(service.clone()), at: at.clone(), state: state.clone(), restarts, folder: folder.clone() });
            let folder = folder.to_string_lossy().into_owned();
            out.rows.push(ServiceRow { service, group, params, folder, start: start.map(When::word), state, restarts, nodes });
        }
    }
    ctx.show(&out);
    for when in [When::Boot, When::Login] {
        if list.iter().any(|e| e.start == when) && !entry::present(when, &home) {
            ctx.out.note(&format!("nothing starts them at {} for this rant and home, `rant enable` sets that up again", when.word()));
        }
    }
    Ok(())
}
