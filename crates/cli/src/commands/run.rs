//! `rant run`: a group, or one node alone, started detached and logging to the logs folder.
//! What it shares with stop, restart and log: where a name runs and how a group is named.

use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use rant_config::{GroupPlan, Settings, Workspace};
use rant_config::param;
use rant_config::plan::{Named, Plan};
use rant_config::refs::same_name;
use rant_process::shown;
use serde::Serialize;

use crate::commands::build::build_for;
use crate::commands::summary::{Row, RowState, Summary, Watch, bring_up, held_by};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::files::path_key;
use crate::mesh::{Client, FULL_SETTLE, Peer};
use crate::names::renames;
use crate::nodes::{self, Session, managed_peers, same_spec, spawn};
use crate::output::{Data, Style, Table, Text};
use crate::pick::{Choice, Picked, pick};
use crate::state::{self, Root, RootKind, State, Store};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group with its key=value params, or `node` and one node type
    #[arg(value_name = "group")]
    pub words: Vec<String>,
    /// show what would run without starting it
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub watch: Watch,
    /// show help, for a group its params
    #[arg(short, long)]
    pub help: bool,
}

/// A group is what a system runs as, so a bare name is a group. One node alone is asked for
/// with the word node, for trying it out. With no name it runs the services.
pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let w = &args.words;
    let help = || {
        crate::cli::print_help("run");
        Ok(())
    };
    match w.first().map(String::as_str) {
        None if args.help => help(),
        None => run_services(ctx, args),
        Some("node") if args.help => help(),
        Some("node") if w.len() != 2 => Err(usage("run node takes one node, such as `rant run node talker`")),
        Some("node") => run_node(ctx, &w[1], args),
        Some(name) => match ctx.require_scope()?.named(name) {
            Named::Group if args.help => group_help(ctx, name),
            Named::Group => run_group(ctx, name, &w[1..], args),
            _ if args.help => help(),
            Named::Node => Err(usage(format!("`{name}` is a node, run it in a group or alone with `rant run node {name}`"))),
            Named::Neither(why) => Err(failed(why)),
        },
    }
}

/// With --domain every node of the plan goes on that domain, whatever its placement says.
pub fn place_on_domain(ctx: &Context, plan: &mut Plan) {
    if let Some(d) = ctx.domain_option {
        plan.instances.iter_mut().for_each(|i| _ = i.env.insert("RANT_DOMAIN".into(), d.to_string()));
    }
}

fn note_renames(ctx: &Context, groups: &[String], plan: &Plan) {
    let nodes: Vec<String> = plan.instances.iter().map(|i| i.name.clone()).collect();
    renames(groups, &nodes).iter().for_each(|n| ctx.out.note(n));
}

/// One node of a node type, named after it. Running the same type again gives the copy a free
/// name, talker_1, then talker_2, since two nodes of one name collide on the mesh.
pub fn run_node(ctx: &mut Context, reference: &str, args: &Args) -> Result<()> {
    let ws = ctx.require_scope()?;
    let mut plan = ws.plan_node(reference)?;
    place_on_domain(ctx, &mut plan);
    note_renames(ctx, &[], &plan);
    if args.dry_run {
        ctx.show(&PlanView::new(&ws, &plan));
        return Ok(());
    }
    let built = args.watch.no_build || build_for(ctx, &[&plan])?;
    let home = ctx.home()?.to_path_buf();
    let settings = &ws.model.settings;
    let inst = &plan.instances[0];
    let mesh = Client::new(ctx.domain()?, Some(&home))?;
    mesh.settle(FULL_SETTLE);
    let mut taken: BTreeSet<String> = mesh.peers().into_iter().map(|p| p.name).collect();
    drop(mesh);

    let rec = {
        let mut s = Session::open(&home, &settings.data)?;
        taken.extend(s.state().instances.iter().map(|i| i.name.clone()));
        let name = (0..).map(|n| if n == 0 { inst.name.clone() } else { format!("{}_{n}", inst.name) }).find(|n| !taken.contains(n)).expect("names never run out");
        let root = Root::node(&name);
        let mut rec = spawn(&home, settings, inst, &name, None)?;
        rec.roots.push(root.key());
        s.state().roots.push(root);
        s.state().instances.push(rec.clone());
        s.save()?;
        rec
    };
    let mut summary = Summary::new(ctx, "", "");
    summary.add(ctx, Row::started(&rec));
    finish_started(ctx, summary, &settings.data, vec![rec], "", "started", &args.watch, built)
}

/// Brings started nodes up and prints the summary, which fails when a node or the build did.
#[expect(clippy::too_many_arguments)]
pub fn finish_started(ctx: &mut Context, mut summary: Summary, data: &Path, started: Vec<state::Instance>, group: &str, verb: &str, w: &Watch, built: bool) -> Result<()> {
    if let Err(e) = bring_up(ctx, &mut summary, data, started, group, verb, w) {
        let _ = summary.finish(ctx);
        return Err(e);
    }
    summary.failed |= !built;
    summary.finish(ctx)
}

fn group_help(ctx: &mut Context, group: &str) -> Result<()> {
    let ws = ctx.require_scope()?;
    let g = ws.group(group)?;
    let mut text = format!("Usage: rant run {}{}", g.file.name, if g.params.is_empty() { "" } else { " [key=value...]" });
    if let Some(d) = &g.description {
        text += &format!("\n\n{d}");
    }
    if !g.params.is_empty() {
        let mut t = Table::default();
        for p in &g.params {
            let mut about = p.default.as_ref().map_or_else(|| "required".to_string(), |d| format!("default {}", param::text(d)));
            if !p.options.is_empty() {
                about += &format!(", one of {}", p.options.iter().map(param::text).collect::<Vec<_>>().join(", "));
            }
            t.row(vec![format!("{}=<{}>", p.name, p.ty.name()), about, p.description.clone().unwrap_or_default()]);
        }
        text += &format!("\n\nParams:\n{}", t.lines("  ").join("\n"));
    }
    ctx.out.line(&text);
    Ok(())
}

/// What a group run would be in the state, the node already running for it, or not.
#[derive(PartialEq)]
enum Action {
    Start,
    /// Running for this very root already.
    Running,
    /// Running for another root, and now for this one too.
    Shared,
    /// On the mesh, started without rant, so left as it is.
    External,
}

/// A group's nodes in plan order, started as run_planned starts groups.
pub fn run_group(ctx: &mut Context, group: &str, words: &[String], args: &Args) -> Result<()> {
    let gp = ctx.require_scope()?.plan_group(group, words)?;
    run_planned(ctx, vec![gp], args)
}

/// A group ready to start: its root, and what each node of its plan does.
struct Prepared {
    group: String,
    root: Root,
    plan: Plan,
    actions: Vec<Action>,
}

/// Checks each node of a group against what runs. One running for another root with the same
/// settings is shared, one on the mesh rant did not start is left as it is. seen holds the nodes
/// of the groups checked before as if they ran, so groups started together share a node.
fn prepare(settings: &Settings, gp: GroupPlan, seen: &mut State, peers: &[Peer], managed: &BTreeMap<u32, String>) -> Result<Prepared> {
    let root = Root::planned(&gp.group, &gp.values);
    let key = root.key();
    let mut actions = Vec::new();
    for inst in &gp.plan.instances {
        if let Some(running) = seen.instance(&inst.name) {
            if !same_spec(running, inst) {
                return Err(failed(format!(
                    "`{}` is already running with other settings for {}, stop it first or give one of them another name",
                    inst.name, running.roots[0]
                )));
            }
            actions.push(if running.roots.contains(&key) { Action::Running } else { Action::Shared });
            continue;
        }
        if peers.iter().any(|p| p.name == inst.name && !managed.contains_key(&p.id)) {
            actions.push(Action::External);
            continue;
        }
        actions.push(Action::Start);
    }
    for (inst, _) in gp.plan.instances.iter().zip(&actions).filter(|(_, a)| **a == Action::Start) {
        let mut rec = nodes::record(settings, inst, &inst.name);
        rec.roots.push(key.clone());
        seen.instances.push(rec);
    }
    Ok(Prepared { group: gp.group, root, plan: gp.plan, actions })
}

/// Starts groups together: every node is checked first, then built once, started, and waited
/// for as one. A group that fails its checks is named and the rest still start.
fn run_planned(ctx: &mut Context, mut gps: Vec<GroupPlan>, args: &Args) -> Result<()> {
    let ws = ctx.require_scope()?;
    for gp in &mut gps {
        place_on_domain(ctx, &mut gp.plan);
        note_renames(ctx, std::slice::from_ref(&gp.group), &gp.plan);
    }
    if args.dry_run {
        let all = Plan { instances: gps.iter().flat_map(|gp| gp.plan.instances.iter().cloned()).collect() };
        ctx.show(&PlanView::new(&ws, &all));
        return Ok(());
    }
    let home = ctx.home()?.to_path_buf();
    let settings = &ws.model.settings;

    let mesh = Client::new(ctx.domain()?, Some(&home))?;
    mesh.settle(FULL_SETTLE);
    let mut s = Session::open(&home, &settings.data)?;
    let managed = managed_peers(&mesh, s.state());
    let peers = mesh.peers();
    drop(mesh);
    let mut seen = s.state().clone();
    let mut prepared = Vec::new();
    let mut failure = None;
    for gp in gps {
        match prepare(settings, gp, &mut seen, &peers, &managed) {
            Ok(p) => prepared.push(p),
            Err(f) => {
                ctx.out.failure(&f);
                failure = Some(Failure::Reported);
            }
        }
    }
    if prepared.is_empty() {
        return Err(Failure::Reported);
    }

    // only what starts is built: a running node's program may be in use
    let starts = |p: &Prepared| p.plan.instances.iter().zip(&p.actions).filter(|(_, a)| **a == Action::Start).map(|(i, _)| i.clone()).collect::<Vec<_>>();
    let starting = Plan { instances: prepared.iter().flat_map(starts).collect() };
    let built = args.watch.no_build || build_for(ctx, &[&starting])?;

    let (header, params) = match prepared.as_slice() {
        [p] => (p.group.clone(), p.root.words().join(" ")),
        _ => (prepared.iter().map(|p| p.root.key()["group ".len()..].to_string()).collect::<Vec<_>>().join(", "), String::new()),
    };
    let mut summary = Summary::new(ctx, &header, &params);
    let mut started = Vec::new();
    for p in &prepared {
        let key = p.root.key();
        if s.state().root(&key).is_none() {
            s.state().roots.push(p.root.clone());
        }
        for (inst, action) in p.plan.instances.iter().zip(&p.actions) {
            let row = match action {
                Action::Start => match spawn(&home, settings, inst, &inst.name, None) {
                    Ok(mut rec) => {
                        rec.roots.push(key.clone());
                        s.state().instances.push(rec.clone());
                        let row = Row::started(&rec);
                        started.push(rec);
                        row
                    }
                    Err(e) => {
                        // what did start stays tracked
                        s.save()?;
                        let _ = summary.finish(ctx);
                        return Err(e);
                    }
                },
                Action::Shared | Action::Running => {
                    let running = s.state().instance_mut(&inst.name).expect("checked above");
                    let with = if *action == Action::Shared { held_by(&running.roots, "shared with") } else { String::new() };
                    if *action == Action::Shared {
                        running.roots.push(key.clone());
                    }
                    if running.group.is_empty() {
                        running.group = inst.group.clone().unwrap_or_default();
                    }
                    Row::new(&inst.name, RowState::Running, "already running", &with)
                }
                Action::External => Row::outside(&inst.name),
            };
            summary.add(ctx, row);
        }
    }
    s.save()?;
    // unlocks the state before the watch
    drop(s);
    let group = if let [p] = prepared.as_slice() { p.group.as_str() } else { "" };
    let ran = finish_started(ctx, summary, &settings.data, started, group, "started", &args.watch, built);
    ran.and(failure.map_or(Ok(()), Err))
}

/// A bare `rant run`: the services of the workspace, or of rant's home outside one, each run as
/// its group from the folder of its file and all started together, as every boot runs them
/// once it is enabled. One that does not plan is named and the rest still start.
pub fn run_services(ctx: &mut Context, args: &Args) -> Result<()> {
    let ws = ctx.require_scope()?;
    if ws.model.settings.services.is_empty() {
        return Err(usage("say which group to run: `rant run <group>`, or one node with `rant run node <node>`"));
    }
    ctx.enter(&ws.model.settings.root);
    let here = ctx.require_scope()?;
    let mut plans = Vec::new();
    let mut failure = None;
    for check in ws.services() {
        match check.checked.map_err(Failure::from).and_then(|c| Ok(here.plan_group(&c.full, &c.words)?)) {
            Ok(gp) => plans.push(gp),
            Err(f) => {
                ctx.out.failure(&f);
                failure = Some(Failure::Reported);
            }
        }
    }
    let ran = if plans.is_empty() { Ok(()) } else { run_planned(ctx, plans, args) };
    ran.and(failure.map_or(Ok(()), Err))
}

/// A group as the workspace names it in full, such as drivers/Base, or the word itself when it
/// names no group.
pub fn group_name(ctx: &mut Context, group: &str) -> Result<String> {
    Ok(ctx.require_scope()?.group(group).map_or_else(|_| group.to_string(), |g| g.file.name))
}

/// A running group's root is the name, in full or by its stem.
fn is_group_of(r: &Root, name: &str) -> bool {
    r.kind == RootKind::Group && (same_name(&r.name, name) || same_name(r.name.rsplit('/').next().unwrap_or(&r.name), name))
}

/// The state here as it stands, empty outside every workspace and home.
pub fn snapshot(ctx: &mut Context) -> Result<State> {
    let Some(data) = ctx.scope().map(|s| s.data.clone()) else { return Ok(State::default()) };
    nodes::snapshot(ctx.home()?, &data)
}

/// Whether stop, restart and log take a bare name as a group: a group of it that runs, else no
/// node of it that runs and a group of it in the config, so the one running wins.
pub fn names_group(ctx: &mut Context, name: &str) -> Result<bool> {
    if ctx.scope().is_none() {
        return Ok(false);
    }
    let st = snapshot(ctx)?;
    if st.roots.iter().any(|r| is_group_of(r, name)) {
        return Ok(true);
    }
    if st.instance(name).is_some() {
        return Ok(false);
    }
    Ok(ctx.require_scope()?.named(name) == Named::Group)
}

/// Every state on this machine rant started nodes in, this workspace's first, with the folder
/// it serves. With prune each waits for its lock and drops what is gone, else it only reads.
pub fn states_anywhere(ctx: &mut Context, prune: bool) -> Result<Vec<(PathBuf, State)>> {
    let home = ctx.home()?.to_path_buf();
    let mut dirs = state::remembered(&home);
    dirs.sort();
    if let Some(here) = ctx.scope() {
        dirs.insert(0, here.data.clone());
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for d in dirs {
        if !seen.insert(path_key(&d)) {
            continue;
        }
        // read and let go at once, so no other command waits on it
        let mut store = Store::open(&d, prune)?;
        if prune {
            state::prune(store.state(), |s| crate::services::running_in(&home, s));
            store.save()?;
        }
        out.push((state::folder_of(&home, &d), store.into_state()));
    }
    Ok(out)
}

/// The folders whose rant runs a group of this name, else a node of it, picked from when several
/// do. Empty when it runs nowhere, None when the pick was given up.
pub fn where_running(ctx: &mut Context, name: &str, node: bool, verb: &str) -> Result<Option<Vec<PathBuf>>> {
    let here = ctx.scope().map(|s| s.root.clone());
    struct Found {
        root: PathBuf,
        groups: Vec<String>,
        nodes: Vec<String>,
    }
    let mut found = Vec::new();
    for (root, st) in states_anywhere(ctx, true)? {
        let groups = if node { Vec::new() } else { st.roots.iter().filter(|r| is_group_of(r, name)).map(|r| r.key()["group ".len()..].to_string()).collect() };
        let nodes: Vec<String> = st.instances.iter().filter(|i| same_name(&i.name, name)).map(|i| i.name.clone()).collect();
        if !groups.is_empty() || !nodes.is_empty() {
            found.push(Found { root, groups, nodes });
        }
    }
    // a group of it running anywhere wins over a node of it
    let groups = found.iter().any(|f| !f.groups.is_empty());
    let runs: Vec<Found> = found.into_iter().filter(|f| if groups { !f.groups.is_empty() } else { !f.nodes.is_empty() }).collect();
    if runs.len() <= 1 {
        return Ok(Some(runs.into_iter().map(|f| f.root).collect()));
    }
    let shown: Vec<String> = runs.iter().map(|f| ctx.shown(&f.root)).collect();
    let choices = runs
        .iter()
        .zip(&shown)
        .map(|(f, label)| Choice {
            label: label.clone(),
            note: if groups { f.groups.join(", ") } else { f.nodes.join(", ") },
            checked: here.as_deref().is_some_and(|h| path_key(h) == path_key(&f.root)),
        })
        .collect();
    let submit = format!("{}{}", verb[..1].to_uppercase(), &verb[1..]);
    let title = format!("{submit} `{name}` in");
    let cannot = || failed(format!("`{name}` runs in {} workspaces: {}, pick one in a terminal", runs.len(), shown.join(", ")));
    if ctx.yes || !std::io::stdin().is_terminal() || !ctx.out.terminal_err() {
        return Err(cannot());
    }
    match pick(&ctx.out, &title, &submit, choices) {
        Picked::Ticked(ticked) => Ok(Some(ticked.into_iter().map(|i| runs[i].root.clone()).collect())),
        Picked::Cancelled => Ok(None),
        Picked::Unavailable => Err(cannot()),
    }
}

/// Runs act once in each folder, as if rant ran there, given the folder as shown when it is
/// not the one rant runs in. The first failure is kept.
pub fn in_each(ctx: &mut Context, roots: &[PathBuf], mut act: impl FnMut(&mut Context, &str) -> Result<()>) -> Result<()> {
    let own = ctx.scope().map(|s| path_key(&s.root));
    let mut first = None;
    for root in roots {
        let shown = if Some(path_key(root)) == own { String::new() } else { ctx.shown(root) };
        ctx.enter(root);
        if let Err(f) = act(ctx, &shown) {
            match first {
                None => first = Some(f),
                Some(_) => ctx.out.failure(&f),
            }
        }
    }
    first.map_or(Ok(()), Err)
}

/// A group's key=value words with each path param made whole from here, so a run planned from
/// another folder means the same. Words the group cannot plan stay, for the plan to refuse.
pub fn resolved_params(ctx: &mut Context, group: &str, words: &[String]) -> Result<Vec<String>> {
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let ws = ctx.require_scope()?;
    let (Ok(gp), Ok(g)) = (ws.plan_group(group, words), ws.group(group)) else { return Ok(words.to_vec()) };
    let path_param = |key: &str| g.params.iter().any(|p| p.name == key && p.ty.name() == "path");
    let whole = |w: &String| match w.split_once('=') {
        Some((key, _)) if path_param(key) => gp.values.get(key).map_or_else(|| w.clone(), |v| format!("{key}={}", param::text(v))),
        _ => w.clone(),
    };
    Ok(words.iter().map(whole).collect())
}

/// A resolved plan for --dry-run: each instance with exactly how it would run, every path
/// absolute, so no other tool needs to resolve anything again.
#[derive(Debug, Serialize)]
pub struct PlanView {
    workspace: String,
    nodes: Vec<PlannedNode>,
}

#[derive(Debug, Serialize)]
struct PlannedNode {
    name: String,
    node: String,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
    restart: rant_config::plan::Restart,
}

impl PlanView {
    pub fn new(ws: &Workspace, plan: &Plan) -> PlanView {
        let node = |i: &rant_config::plan::Instance| PlannedNode {
            name: i.name.clone(),
            node: i.type_ref(),
            argv: i.argv.clone(),
            env: i.env.clone(),
            cwd: rant_config::paths::slash(&i.cwd),
            restart: i.restart,
        };
        PlanView { workspace: rant_config::paths::slash(&ws.model.settings.root), nodes: plan.instances.iter().map(node).collect() }
    }
}

impl Data for PlanView {
    fn text(&self, t: &Text) -> String {
        let mut lines = Vec::new();
        for n in &self.nodes {
            lines.push(format!("{}  {}", t.paint(Style::Bold, &n.name), t.paint(Style::Dim, &n.node)));
            lines.push(format!("  run  {}", shown(&n.argv)));
            lines.push(format!("  cwd  {}", n.cwd));
            lines.extend(n.env.iter().map(|(k, v)| format!("  env  {k}={v}")));
            if n.restart != rant_config::plan::Restart::Never {
                lines.push(format!("  restart  {}", n.restart.as_str()));
            }
        }
        lines.join("\n")
    }
}

/// The summary of what stopped, closed by its counts.
pub fn closed(ctx: &Context, mut summary: Summary) -> Result<()> {
    summary.report.closing = summary.report.counts();
    summary.finish(ctx)
}

/// An empty summary that says only why nothing happened.
pub fn nothing(ctx: &Context, why: &str) -> Result<()> {
    let mut summary = Summary::new(ctx, "", "");
    summary.report.closing = why.to_string();
    summary.finish(ctx)
}
