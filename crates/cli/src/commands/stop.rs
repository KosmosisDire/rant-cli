//! `rant stop`: a group, one node, everything this workspace started, or every node on this
//! machine. A service keeps running unless it is named.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rant_config::refs::same_name;
use rant_process::Stopped;

use crate::commands::run::{closed, group_name, in_each, names_group, nothing, resolved_params, where_running};
use crate::commands::summary::{Row, RowState, Summary, held_by};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::files::path_key;
use crate::mesh::{Client, FULL_SETTLE};
use crate::nodes::{GRACE, Session, halt, release_root};
use crate::services;
use crate::state::{self, Instance, Root, RootKind, State, Store};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group with its key=value params, or a node, after `node` to say so
    #[arg(value_name = "group")]
    pub words: Vec<String>,
    /// stop every node on this machine but the services, whoever started it
    #[arg(short, long)]
    pub all: bool,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let w = &args.words;
    if args.all {
        if !w.is_empty() {
            return Err(usage("--all takes no group or node"));
        }
        return stop_everything(ctx);
    }
    match w.first().map(String::as_str) {
        None if ctx.workspace().is_some() => stop_workspace(ctx),
        None => stop_everything(ctx),
        Some("node") if w.len() != 2 => Err(usage("stop node takes one node, such as `rant stop node talker`")),
        Some("node") => stop_node(ctx, &w[1]),
        Some(name) => stop_named(ctx, name, &w[1..]),
    }
}

fn stop_named(ctx: &mut Context, name: &str, words: &[String]) -> Result<()> {
    let Some(where_) = where_running(ctx, name, false, "stop")? else { return Err(Failure::Reported) };
    if where_.is_empty() {
        // it runs nowhere rant started it: a group says so, a node may run without rant
        if names_group(ctx, name)? {
            return stop_group(ctx, name, words);
        }
        if !words.is_empty() {
            return Err(usage("a node takes no key=value, only a group does"));
        }
        return stop_loose(ctx, name);
    }
    let words = resolved_params(ctx, name, words)?;
    in_each(ctx, &where_, |ctx, in_| {
        if names_group(ctx, name)? {
            if !in_.is_empty() {
                ctx.out.note(&format!("`{name}` runs in {in_}"));
            }
            return stop_group(ctx, name, &words);
        }
        if !words.is_empty() {
            return Err(usage("a node takes no key=value, only a group does"));
        }
        stop_here(ctx, name, in_)
    })
}

/// What stopping or restarting everything leaves be in an enabled workspace, since only naming
/// a service stops it: its services' roots and nodes, and the running ones by name.
#[derive(Debug, Default)]
pub struct Kept {
    pub roots: BTreeSet<String>,
    pub nodes: BTreeSet<String>,
    pub running: Vec<String>,
}

/// st is the state of the workspace whose data folder is data.
pub fn kept_services(home: &Path, st: &State, data: &Path) -> Result<Kept> {
    let mut k = Kept::default();
    let folder = state::folder_of(home, data);
    if !services::enabled(home)?.iter().any(|e| path_key(&e.folder) == path_key(&folder)) {
        return Ok(k);
    }
    // a folder that is no longer a workspace holds no services
    let Some(settings) = rant_config::Settings::find(&folder, home).ok().filter(|s| path_key(&s.root) == path_key(&folder)) else { return Ok(k) };
    let (ws, _) = rant_config::Workspace::load(settings, &folder, home);
    for check in ws.services() {
        let Ok(c) = check.checked else { continue };
        let params = c.values.iter().map(|(k, v)| (k.clone(), rant_config::param::text(v))).collect();
        let key = Root::group(&c.full, params).key();
        let holding: Vec<&Instance> = st.instances.iter().filter(|i| i.roots.contains(&key)).collect();
        k.nodes.extend(holding.iter().map(|i| i.name.clone()));
        if !holding.is_empty() {
            k.running.push(std::iter::once(&check.service.group).chain(&c.words).cloned().collect::<Vec<_>>().join(" "));
        }
        k.roots.insert(key);
    }
    Ok(k)
}

/// Drops every node and root of a state but those its services hold, the roots from the nodes
/// left running too.
fn keep_only(st: &mut State, k: &Kept) {
    st.instances.retain(|i| k.nodes.contains(&i.name));
    for i in &mut st.instances {
        i.roots.retain(|r| k.roots.contains(r));
    }
    st.roots.retain(|r| k.roots.contains(&r.key()));
}

/// Stops every node of a state but those its services hold, last started first.
fn stop_all_but(ctx: &Context, home: &Path, summary: &mut Summary, st: &mut State, k: &Kept) {
    for i in st.instances.iter().rev().filter(|i| !k.nodes.contains(&i.name)) {
        summary.add(ctx, Row::stopped(&i.name, halt(home, i)));
    }
    keep_only(st, k);
}

fn note_kept(ctx: &Context, running: &[String]) {
    if !running.is_empty() {
        ctx.out.note(&format!("services keep running, name one to stop it: {}", running.join(", ")));
    }
}

/// "Stop all 3 nodes?"
fn stop_all_question(n: usize) -> String {
    format!("Stop all {n} {}?", if n == 1 { "node" } else { "nodes" })
}

/// Stops everything rant started in this workspace but its services, last started first, once
/// the list has been shown and the question answered.
fn stop_workspace(ctx: &mut Context) -> Result<()> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let data = &ws.model.settings.data;
    let mut s = Session::open(&home, data)?;
    let k = kept_services(&home, s.state(), data)?;
    let st = s.state();
    let names: Vec<String> = st.instances.iter().filter(|i| !k.nodes.contains(&i.name)).map(|i| i.name.clone()).collect();
    let mut groups = Vec::new();
    for r in st.roots.iter().filter(|r| r.kind == RootKind::Group && !k.roots.contains(&r.key())) {
        // params tell runs of one group apart, as ls shows them
        let runs = st.roots.iter().filter(|o| o.kind == RootKind::Group && o.name == r.name).count();
        groups.push(if runs > 1 { r.key()["group ".len()..].to_string() } else { r.name.clone() });
    }
    if names.is_empty() {
        keep_only(s.state(), &k);
        s.save()?;
        drop(s);
        let why = if k.running.is_empty() { "nothing started here is running".to_string() } else { format!("only services run here, name one to stop it: {}", k.running.join(", ")) };
        return nothing(ctx, &why);
    }
    if !groups.is_empty() {
        ctx.out.note(&format!("groups: {}", groups.join(", ")));
    }
    ctx.out.note(&format!("nodes:  {}", names.join(", ")));
    note_kept(ctx, &k.running);
    if !ctx.confirm(&stop_all_question(names.len())) {
        return Err(Failure::Reported);
    }
    let mut summary = Summary::new(ctx, "", "");
    stop_all_but(ctx, &home, &mut summary, s.state(), &k);
    s.save()?;
    closed(ctx, summary)
}

/// One workspace's state, locked until this command ends, and what its services hold.
struct Held {
    root: PathBuf,
    store: Store,
    kept: Kept,
}

/// Every workspace on this machine with nodes rant started, this one first. The rest lock in
/// path order, so two stops never wait on each other.
fn started_anywhere(ctx: &mut Context) -> Result<Vec<Held>> {
    let home = ctx.home()?.to_path_buf();
    let mut dirs = state::remembered(&home);
    dirs.sort();
    if let Some(ws) = ctx.scope() {
        dirs.insert(0, ws.data.clone());
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for d in dirs {
        if !seen.insert(path_key(&d)) {
            continue;
        }
        let mut store = Store::open(&d, true)?;
        state::prune(store.state(), |s| services::running_in(&home, s));
        store.save()?;
        let kept = kept_services(&home, store.state(), &d)?;
        if !store.state().instances.is_empty() {
            out.push(Held { root: state::folder_of(&home, &d), store, kept });
        }
    }
    Ok(out)
}

/// A node on this machine that no workspace started, found on the mesh by its address.
struct Loose {
    name: String,
    pid: u64,
}

/// The mesh's nodes on this machine that no held workspace started, only those named `only`
/// when given. A node that does not report its process id cannot be stopped and is named.
fn loose_here(ctx: &Context, mesh: &Client, held: &mut [Held], only: Option<&str>) -> Vec<Loose> {
    let names: BTreeMap<u32, String> = mesh.peers().into_iter().filter(|p| p.here && only.is_none_or(|o| same_name(&p.name, o))).map(|p| (p.id, p.name)).collect();
    let pids = mesh.pids(&names.keys().copied().collect::<Vec<_>>(), Duration::from_millis(500));
    let mut out = Vec::new();
    for (id, name) in names {
        let Some(&pid) = pids.get(&id) else {
            ctx.out.warn(&format!("`{name}` did not report its pid, cannot stop it"));
            continue;
        };
        let started = held.iter_mut().any(|h| h.store.state().instances.iter().any(|i| rant_process::owns(&i.tracking(), pid)));
        if !started {
            out.push(Loose { name, pid });
        }
    }
    out
}

fn stopped_loose(l: &Loose) -> Row {
    let how = rant_process::stop_pid(l.pid, GRACE).unwrap_or(Stopped::AlreadyGone);
    let mut row = Row::stopped(&l.name, how);
    row.detail = [row.detail.as_str(), "started outside rant"].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
    row
}

/// Stops every node on this machine but the services: what any workspace started, on any
/// domain, and what runs here without rant on the domain in use.
fn stop_everything(ctx: &mut Context) -> Result<()> {
    let mut held = started_anywhere(ctx)?;
    let home = ctx.home()?.to_path_buf();
    let mesh = Client::new(ctx.domain()?, Some(&home))?;
    mesh.settle(FULL_SETTLE);
    let loose = loose_here(ctx, &mesh, &mut held, None);
    drop(mesh);

    let mut rows: Vec<(String, String)> = Vec::new();
    let mut kept = Vec::new();
    let mut count = loose.len();
    for h in &mut held {
        kept.extend(h.kept.running.iter().cloned());
        let names: Vec<String> = h.store.state().instances.iter().filter(|i| !h.kept.nodes.contains(&i.name)).map(|i| i.name.clone()).collect();
        if !names.is_empty() {
            count += names.len();
            rows.push((ctx.shown(&h.root), names.join(", ")));
        }
    }
    if !loose.is_empty() {
        rows.push(("not started by rant".into(), loose.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if count == 0 {
        let why = if kept.is_empty() { "no node is running on this machine".to_string() } else { format!("only services run on this machine, name one to stop it: {}", kept.join(", ")) };
        return nothing(ctx, &why);
    }
    let width = rows.iter().map(|(w, _)| w.len()).max().unwrap_or(0);
    for (where_, names) in &rows {
        ctx.out.note(&format!("{where_}:{}{names}", " ".repeat(width - where_.len() + 2)));
    }
    note_kept(ctx, &kept);
    if !ctx.confirm(&stop_all_question(count)) {
        return Err(Failure::Reported);
    }
    let mut summary = Summary::new(ctx, "", "");
    for h in &mut held {
        stop_all_but(ctx, &home, &mut summary, h.store.state(), &h.kept);
        h.store.save()?;
    }
    for l in &loose {
        summary.add(ctx, stopped_loose(l));
    }
    closed(ctx, summary)
}

/// Stops the node of that name this workspace's rant started, asking first when a running
/// group holds it. in_ names the workspace when it is not the one rant runs in.
fn stop_here(ctx: &mut Context, name: &str, in_: &str) -> Result<()> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let mut s = Session::open(&home, &ws.model.settings.data)?;
    let inst = s.state().instance(name).cloned().ok_or_else(|| failed(format!("`{name}` stopped already")))?;
    let groups: Vec<&str> = inst.roots.iter().filter_map(|k| k.strip_prefix("group ")).collect();
    let where_ = if in_.is_empty() { String::new() } else { format!(" in {in_}") };
    if !groups.is_empty() && !ctx.confirm(&format!("`{}` belongs to the running group {}{where_}. Stop it anyway?", inst.name, groups.join(", "))) {
        return Err(Failure::Reported);
    }
    let mut row = Row::stopped(&inst.name, halt(&home, &inst));
    if !in_.is_empty() {
        row.detail = [row.detail.clone(), format!("started in {in_}")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    }
    s.state().forget(&inst.name);
    s.save()?;
    drop(s);
    let mut summary = Summary::new(ctx, "", "");
    summary.add(ctx, row);
    closed(ctx, summary)
}

/// Stops a node on this machine no workspace's rant started, found on the mesh, else says
/// where it runs or that it runs nowhere.
fn stop_loose(ctx: &mut Context, name: &str) -> Result<()> {
    let mut held = started_anywhere(ctx)?;
    let domain = ctx.domain()?;
    let mesh = Client::new(domain, Some(ctx.home()?))?;
    mesh.settle(FULL_SETTLE);
    let loose = loose_here(ctx, &mesh, &mut held, Some(name));
    if !loose.is_empty() {
        let mut summary = Summary::new(ctx, "", "");
        for l in &loose {
            summary.add(ctx, stopped_loose(l));
        }
        return closed(ctx, summary);
    }
    if let Some(p) = mesh.peers().iter().find(|p| same_name(&p.name, name) && !p.here) {
        return Err(failed(format!("`{name}` is running on {}, rant only stops nodes on this machine", p.host())));
    }
    Err(failed(format!("no node `{name}` on domain {domain}, see `rant ls nodes`")))
}

/// Stops one node wherever on this machine rant started it, picking when several workspaces
/// did, else one rant did not start.
pub fn stop_node(ctx: &mut Context, name: &str) -> Result<()> {
    let Some(where_) = where_running(ctx, name, true, "stop")? else { return Err(Failure::Reported) };
    if where_.is_empty() {
        return stop_loose(ctx, name);
    }
    in_each(ctx, &where_, |ctx, in_| stop_here(ctx, name, in_))
}

/// The group roots to stop: the one root params name, else every root of the group. The
/// group's own name is looked up, so a bare name finds a group in a package.
fn targets(ctx: &mut Context, st: &State, group: &str, words: &[String]) -> Result<Vec<Root>> {
    if !words.is_empty() {
        let gp = ctx.require_scope()?.plan_group(group, words)?;
        let params = gp.values.iter().map(|(k, v)| (k.clone(), rant_config::param::text(v))).collect();
        let root = Root::group(&gp.group, params);
        if st.root(&root.key()).is_none() {
            return Err(failed(format!("`{}` is not running, see `rant ls`", &root.key()["group ".len()..])));
        }
        return Ok(vec![root]);
    }
    let name = group_name(ctx, group)?;
    let out: Vec<Root> = st.roots.iter().filter(|r| r.kind == RootKind::Group && r.name == name).cloned().collect();
    if out.is_empty() {
        return Err(failed(format!("group `{name}` is not running, see `rant ls`")));
    }
    Ok(out)
}

/// Stops what the group started. A node of the same name it did not start keeps running. A
/// group that runs only inside others stops its own nodes there.
pub fn stop_group(ctx: &mut Context, group: &str, words: &[String]) -> Result<()> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let mut s = Session::open(&home, &ws.model.settings.data)?;
    if words.is_empty() {
        let name = group_name(ctx, group)?;
        let own = s.state().roots.iter().any(|r| r.kind == RootKind::Group && r.name == name);
        let inside = s.state().made_by(&name);
        if !own && !inside.is_empty() {
            return stop_inside(ctx, &home, s, &name, &inside);
        }
    }
    let roots = targets(ctx, s.state(), group, words)?;
    let params = if roots.len() == 1 { roots[0].words().join(" ") } else { String::new() };
    let mut summary = Summary::new(ctx, &roots[0].name, &params);
    for r in &roots {
        for rel in release_root(&home, s.state(), &r.key()) {
            let row = if rel.kept_for.is_empty() {
                Row::stopped(&rel.name, rel.how)
            } else {
                Row::new(&rel.name, RowState::Running, "kept running", &held_by(&rel.kept_for, "still needed by"))
            };
            summary.add(ctx, row);
        }
        s.save()?;
    }
    drop(s);
    closed(ctx, summary)
}

/// Stops the nodes a group's own blocks made inside the runs of the groups that include it,
/// last started first, whichever of them hold the nodes.
fn stop_inside(ctx: &Context, home: &Path, mut s: Session, group: &str, names: &[String]) -> Result<()> {
    let mut summary = Summary::new(ctx, group, "");
    for name in names.iter().rev() {
        let inst = s.state().instance(name).cloned().expect("made_by names instances");
        let mut row = Row::stopped(name, halt(home, &inst));
        if row.detail.is_empty() {
            row.detail = held_by(&inst.roots, "inside");
        }
        s.state().forget(name);
        summary.add(ctx, row);
    }
    s.save()?;
    drop(s);
    closed(ctx, summary)
}
