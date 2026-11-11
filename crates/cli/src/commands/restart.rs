//! `rant restart`: stop and start again as configured now, start what is not running, all of
//! it when nothing is named.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rant_config::plan::Plan;
use rant_process::Stopped;

use crate::commands::build::build_for;
use crate::commands::run::{self, finish_started, group_name, in_each, names_group, nothing, place_on_domain, resolved_params, snapshot, states_anywhere, where_running};
use crate::commands::enable::kept_services;
use crate::commands::summary::{Row, RowState, Summary, Watch, held_by};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::mesh::{Client, FULL_SETTLE};
use crate::nodes::{Session, halt, managed_peers, spawn};
use crate::state::{Instance, Root, RootKind, State};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group with its key=value params, or a node, after `node` to say so
    #[arg(value_name = "group")]
    pub words: Vec<String>,
    #[command(flatten)]
    pub watch: Watch,
}

/// Restarts a name where it runs on this machine, as stop finds it, else starts it here.
pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let w = &args.words;
    if w.is_empty() {
        return restart_all(ctx, args);
    }
    let mut node = w[0] == "node";
    if node && w.len() != 2 {
        return Err(usage("restart node takes one node, such as `rant restart node talker`"));
    }
    let mut name = if node { w[1].clone() } else { w[0].clone() };
    let Some(mut where_) = where_running(ctx, &name, node, "restart")? else { return Err(Failure::Reported) };
    if where_.is_empty()
        && (node || (w.len() == 1 && !names_group(ctx, &name)?))
        && let Some(running) = running_as(ctx, &name)?
    {
        (name, node) = (running, true);
        let Some(again) = where_running(ctx, &name, node, "restart")? else { return Err(Failure::Reported) };
        where_ = again;
    }
    let words = if node { Vec::new() } else { resolved_params(ctx, &name, &w[1..])? };
    let act = |ctx: &mut Context, in_: &str| {
        if !in_.is_empty() {
            ctx.out.note(&format!("`{name}` runs in {in_}"));
        }
        if node { restart_node(ctx, &name, args) } else { restart_named(ctx, &name, &words, args) }
    };
    if where_.is_empty() {
        return act(ctx, "");
    }
    in_each(ctx, &where_, act)
}

/// The plan a root runs now, from the config as it stands, so a restart takes in edits and new
/// builds. Planned before anything stops, so a broken config stops nothing.
fn replan(ctx: &mut Context, root: &Root, node: Option<&Instance>) -> Result<Plan> {
    let ws = ctx.require_scope()?;
    let mut plan = match root.kind {
        RootKind::Group => ws.plan_group(&root.name, &root.words())?.plan,
        RootKind::Node => ws.plan_node(node.map_or(&root.name, |n| &n.node_type))?,
    };
    place_on_domain(ctx, &mut plan);
    Ok(plan)
}

/// A root to restart: its plan as configured now, the one node of it when only that, and the
/// nodes a service holds, which a restart of everything leaves be.
struct Target {
    root: Root,
    plan: Plan,
    only: Option<String>,
    keep: BTreeSet<String>,
}

/// What a restart stopped of one root, to start again once the build is done.
struct Halted<'a> {
    target: &'a Target,
    how: BTreeMap<String, Stopped>,
    /// Nodes the plan no longer has.
    dropped: Vec<Row>,
}

/// Stops a root's nodes, last started first. A node the plan has dropped stops for good unless
/// another group needs it.
fn halt_root<'a>(home: &Path, s: &mut Session, t: &'a Target) -> Result<Halted<'a>> {
    let mut h = Halted { target: t, how: BTreeMap::new(), dropped: Vec::new() };
    let key = t.root.key();
    let st = s.state();
    let mine: Vec<String> = st
        .instances
        .iter()
        .filter(|i| i.roots.contains(&key) && t.only.as_ref().is_none_or(|o| &i.name == o) && !t.keep.contains(&i.name))
        .map(|i| i.name.clone())
        .collect();
    for name in mine.iter().rev() {
        let planned = t.plan.instances.iter().any(|p| &p.name == name);
        let inst = st.instance_mut(name).expect("listed above");
        if !planned {
            inst.roots.retain(|k| k != &key);
            if inst.held_by_group() {
                // another group still needs it as it is
                continue;
            }
        }
        let inst = inst.clone();
        h.how.insert(name.clone(), halt(home, &inst));
        if !planned {
            st.forget(name);
            h.dropped.push(Row::new(name, RowState::Stopped, "stopped", &format!("no longer in {}", t.root.name)));
        }
    }
    s.save()?;
    Ok(h)
}

/// Starts a halted root's plan in order, each node under its old name and roots.
fn start_root(ctx: &mut Context, home: &Path, s: &mut Session, h: Halted, started: &mut Vec<Instance>, summary: &mut Summary) -> Result<()> {
    for r in h.dropped {
        summary.add(ctx, r);
    }
    let ws = ctx.require_scope()?;
    let key = h.target.root.key();
    let planned = &h.target.plan.instances;
    // a node the plan has that rant does not run may run on the mesh without rant
    let mut elsewhere = BTreeSet::new();
    if planned.iter().any(|p| s.state().instance(&p.name).is_none()) {
        let mesh = Client::new(ctx.domain()?, Some(home))?;
        mesh.settle(FULL_SETTLE);
        let managed = managed_peers(&mesh, s.state());
        elsewhere.extend(mesh.peers().into_iter().filter(|p| !managed.contains_key(&p.id)).map(|p| p.name));
    }
    for p in planned {
        let inst = s.state().instance(&p.name).cloned();
        if inst.is_some() && h.target.keep.contains(&p.name) {
            summary.add(ctx, Row::new(&p.name, RowState::Running, "kept running", "a service holds it"));
            continue;
        }
        if let Some(i) = inst.as_ref().filter(|i| !i.roots.contains(&key)) {
            // another root's node, now shared, left running as it is
            let with = held_by(&i.roots, "shared with");
            s.state().instance_mut(&p.name).expect("found above").roots.push(key.clone());
            summary.add(ctx, Row::new(&p.name, RowState::Running, "already running", &with));
            continue;
        }
        if inst.is_none() && elsewhere.contains(&p.name) {
            summary.add(ctx, Row::outside(&p.name));
            continue;
        }
        // a node roots share starts once, a second start would leave the first untracked
        if started.iter().any(|i| i.name == p.name) {
            continue;
        }
        let roots = inst.as_ref().map_or_else(|| vec![key.clone()], |i| i.roots.clone());
        // a node comes back in the session it ran in
        let mut rec = spawn(home, &ws.model.settings, p, &p.name, inst.as_ref().map(|i| i.session))?;
        rec.roots = roots;
        match s.state().instance_mut(&p.name) {
            Some(old) => *old = rec.clone(),
            None => s.state().instances.push(rec.clone()),
        }
        s.save()?;
        let mut row = Row::started(&rec);
        if matches!(h.how.get(&p.name), Some(Stopped::Killed | Stopped::KilledAtOnce)) {
            row.detail = "the old one killed".into();
        }
        summary.add(ctx, row);
        started.push(rec);
    }
    Ok(())
}

/// Stops the targets, builds them, then starts them again, with the state locked throughout so
/// nothing else starts these nodes meanwhile. Gives whether the build worked.
fn restart(ctx: &mut Context, s: &mut Session, targets: &[Target], started: &mut Vec<Instance>, summary: &mut Summary, w: &Watch) -> Result<bool> {
    let home = ctx.home()?.to_path_buf();
    let mut halted = Vec::new();
    for t in targets {
        halted.push(halt_root(&home, s, t)?);
    }
    let plans: Vec<&Plan> = targets.iter().map(|t| &t.plan).collect();
    let built = w.no_build || build_for(ctx, &plans)?;
    for h in halted {
        start_root(ctx, &home, s, h, started, summary)?;
    }
    Ok(built)
}

/// Runs a restart with the state locked, then waits for what started once it is unlocked, so
/// other commands need not wait.
fn restart_then_watch(ctx: &mut Context, header: &str, w: &Watch, targets: impl FnOnce(&mut Context, &mut Session) -> Result<Vec<Target>>) -> Result<()> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let data = ws.model.settings.data.clone();
    let mut summary = Summary::new(ctx, header, "");
    let mut started = Vec::new();
    let built = {
        let mut s = Session::open(&home, &data)?;
        let targets = targets(ctx, &mut s)?;
        match restart(ctx, &mut s, &targets, &mut started, &mut summary, w) {
            Ok(built) => built,
            Err(e) => {
                let _ = summary.finish(ctx);
                return Err(e);
            }
        }
    };
    finish_started(ctx, summary, &data, started, header, "restarted", w, built)
}

/// The running roots of a group: the one its params name, else every one of it. Empty when it
/// is not running.
fn group_roots(ctx: &mut Context, st: &State, group: &str, words: &[String]) -> Result<Vec<Root>> {
    if !words.is_empty() {
        let gp = ctx.require_scope()?.plan_group(group, words)?;
        let key = Root::planned(&gp.group, &gp.values).key();
        return Ok(st.roots.iter().filter(|r| r.key() == key).cloned().collect());
    }
    let name = group_name(ctx, group)?;
    Ok(st.roots.iter().filter(|r| r.kind == RootKind::Group && r.name == name).cloned().collect())
}

/// Every root this workspace started but its services, each planned before anything stops, so
/// a broken config stops nothing.
fn restart_all(ctx: &mut Context, args: &Args) -> Result<()> {
    let now = snapshot(ctx)?;
    let home = ctx.home()?.to_path_buf();
    let data = ctx.require_scope()?.model.settings.data.clone();
    let k = kept_services(&home, &now, &data)?;
    let keys: Vec<String> = now.roots.iter().map(Root::key).filter(|key| !k.roots.contains(key)).collect();
    if keys.is_empty() {
        let why = if k.running.is_empty() { "nothing rant started is running".to_string() } else { format!("only services run here, name one to restart it: {}", k.running.join(", ")) };
        return nothing(ctx, &why);
    }
    ctx.out.note(&format!("roots: {}", keys.join(", ")));
    if !k.running.is_empty() {
        ctx.out.note(&format!("services keep running, name one to restart it: {}", k.running.join(", ")));
    }
    let count = now.instances.iter().filter(|i| !k.nodes.contains(&i.name)).count();
    if !ctx.confirm(&format!("Restart all {count} nodes?")) {
        return Err(Failure::Reported);
    }
    restart_then_watch(ctx, "", &args.watch, |ctx, s| {
        let held = kept_services(&home, s.state(), &data)?;
        let mut targets = Vec::new();
        for r in s.state().roots.clone() {
            if held.roots.contains(&r.key()) {
                continue;
            }
            if r.kind == RootKind::Group {
                targets.push(Target { plan: replan(ctx, &r, None)?, root: r, only: None, keep: held.nodes.clone() });
                continue;
            }
            let first = s.state().instances.iter().find(|i| i.roots.first() == Some(&r.key())).cloned();
            if let Some(i) = first.filter(|i| !held.nodes.contains(&i.name)) {
                targets.push(Target { plan: replan(ctx, &r, Some(&i))?, root: r, only: Some(i.name), keep: BTreeSet::new() });
            }
        }
        Ok(targets)
    })
}

/// A running node alone, as the root that started it plans it now.
fn node_target(ctx: &mut Context, st: &State, inst: &Instance) -> Result<Target> {
    let root = st.root(&inst.roots[0]).cloned().ok_or_else(|| failed(format!("rant lost track of `{}`, stop and start it again", inst.name)))?;
    let mut plan = replan(ctx, &root, Some(inst))?;
    if root.kind == RootKind::Group {
        plan.instances.retain(|i| i.name == inst.name);
    }
    if plan.instances.is_empty() {
        return Err(failed(format!("`{}` is no longer in {}, stop it instead", inst.name, &root.key()["group ".len()..])));
    }
    Ok(Target { root, plan, only: Some(inst.name.clone()), keep: BTreeSet::new() })
}

/// Running nodes by name, each under its name and roots. header heads the summary.
fn restart_nodes(ctx: &mut Context, names: &[String], header: &str, w: &Watch) -> Result<()> {
    restart_then_watch(ctx, header, w, |ctx, s| {
        let mut targets = Vec::new();
        for name in names {
            let inst = s.state().instance(name).cloned().ok_or_else(|| failed(format!("no node `{name}` started here, see `rant ls nodes`")))?;
            targets.push(node_target(ctx, s.state(), &inst)?);
        }
        Ok(targets)
    })
}

/// One node by name, started when it is not running.
fn restart_node(ctx: &mut Context, name: &str, args: &Args) -> Result<()> {
    if let Some(i) = snapshot(ctx)?.instance(name) {
        return restart_nodes(ctx, std::slice::from_ref(&i.name), "", &args.watch);
    }
    ctx.out.note(&format!("`{name}` is not running, starting it"));
    run::run_node(ctx, name, &run_args(args))
}

fn run_args(args: &Args) -> run::Args {
    run::Args { words: Vec::new(), dry_run: false, watch: args.watch.clone(), help: false }
}

/// A bare name restarted in this workspace: a group of it, else the node, started when it is
/// not running.
fn restart_named(ctx: &mut Context, name: &str, words: &[String], args: &Args) -> Result<()> {
    // what is not running starts, looked up first without holding the state, which run takes
    if !names_group(ctx, name)? {
        if !words.is_empty() {
            return Err(usage("a node takes no key=value, only a group does"));
        }
        return restart_node(ctx, name, args);
    }
    let now = snapshot(ctx)?;
    if group_roots(ctx, &now, name, words)?.is_empty() {
        // a group that runs only inside others restarts its own nodes there
        let full = group_name(ctx, name)?;
        let inside = now.made_by(&full);
        if words.is_empty() && !inside.is_empty() {
            return restart_nodes(ctx, &inside, &full, &args.watch);
        }
        ctx.out.note(&format!("group `{name}` is not running, starting it"));
        return run::run_group(ctx, name, words, &run_args(args));
    }
    let header = group_roots(ctx, &now, name, words)?[0].name.clone();
    restart_then_watch(ctx, &header, &args.watch, |ctx, s| {
        let roots = group_roots(ctx, &s.state().clone(), name, words)?;
        roots.into_iter().map(|r| Ok(Target { plan: replan(ctx, &r, None)?, root: r, only: None, keep: BTreeSet::new() })).collect()
    })
}

/// The node a node type's reference means: the one running under the name that type starts
/// with on this machine. Another under that name or of that type is not it.
fn running_as(ctx: &mut Context, reference: &str) -> Result<Option<String>> {
    let Ok(plan) = ctx.require_scope()?.plan_node(reference) else { return Ok(None) };
    let p = &plan.instances[0];
    let ty = p.type_ref();
    for (_, st) in states_anywhere(ctx, false)? {
        if let Some(i) = st.instances.iter().find(|i| i.name == p.name && i.node_type == ty) {
            return Ok(Some(i.name.clone()));
        }
    }
    Ok(None)
}
