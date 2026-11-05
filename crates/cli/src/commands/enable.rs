//! `rant enable`: the services of a workspace, or of rant's home outside one, run now and at
//! every boot or login. What disable, stop, restart and `ls services` share about them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rant_config::plan::Named;
use rant_config::{ServiceCheck, Settings, Workspace};

use crate::commands::run::{self, run_group, run_services};
use crate::commands::summary::Watch;
use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::files::same_path;
use crate::services::{self, Enabled, When, entry};
use crate::state::{self, Root, State};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group with its key=value params, added to the services first
    #[arg(value_name = "group")]
    pub words: Vec<String>,
    /// start them when you log in instead, for what needs your desktop
    #[arg(long)]
    pub login: bool,
    /// show what would run without starting or keeping it
    #[arg(long)]
    pub dry_run: bool,
    #[command(flatten)]
    pub watch: Watch,
}

/// What messages call the scope whose services enable and disable act on.
pub fn scope_name(s: &Settings) -> &'static str {
    if s.home { "rant's home" } else { "this workspace" }
}

pub fn when_text(when: When) -> &'static str {
    match when {
        When::Boot => "when this computer starts",
        When::Login => "when you log in",
    }
}

/// The key of the root a service runs under, started as rant run starts it from the folder of
/// its file, or what keeps it from planning.
pub fn service_key(check: &ServiceCheck) -> std::result::Result<String, String> {
    let c = check.checked.as_ref().map_err(ToString::to_string)?;
    Ok(Root::planned(&c.full, &c.values).key())
}

/// The workspace or rant's home whose root is folder, with its services, or why there is none.
pub fn scope_at(home: &Path, folder: &Path) -> std::result::Result<Workspace, String> {
    let settings = Settings::find(folder, home).map_err(|d| d.to_string())?;
    if !same_path(&settings.root, folder) {
        return Err("no workspace there now".into());
    }
    Ok(Workspace::load(settings, folder, home).0)
}

/// Saves the list, and drops the login entry once nothing starts at login. The boot entry
/// stays, so enabling again never asks for admin.
pub fn keep(home: &Path, list: &[Enabled]) -> Result<()> {
    services::save(home, list)?;
    if !list.iter().any(|e| e.start == When::Login) {
        entry::remove(When::Login, home)?;
    }
    Ok(())
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
    if !services::enabled(home)?.iter().any(|e| same_path(&e.folder, &folder)) {
        return Ok(k);
    }
    let Ok(ws) = scope_at(home, &folder) else { return Ok(k) };
    for check in ws.services() {
        let Ok(key) = service_key(&check) else { continue };
        let holding: Vec<&state::Instance> = st.instances.iter().filter(|i| i.roots.contains(&key)).collect();
        k.nodes.extend(holding.iter().map(|i| i.name.clone()));
        if !holding.is_empty() {
            k.running.push(check.shown());
        }
        k.roots.insert(key);
    }
    Ok(k)
}

/// The placement variables of the shell, which the plans at boot or login need too.
fn placement_env() -> BTreeMap<String, String> {
    let set = |n: &&str| Some((n.to_string(), std::env::var(n).ok().filter(|v| !v.is_empty())?));
    rant_config::placement::ENV.iter().filter_map(set).collect()
}

/// Writes the entry that starts it, saying first when the boot one asks for admin.
fn write_entry(ctx: &Context, home: &Path, when: When) -> Result<()> {
    if when == When::Boot && !entry::present(when, home) {
        ctx.out.note(&format!("starting at boot takes admin once, to add {}", entry::boot_place()));
    }
    entry::write(when, home)
}

/// Enables the scope with the start, domain and placement of this command, and writes the entry
/// that starts it. Says what changed, unless only a service joined it.
fn enable_scope(ctx: &Context, s: &Settings, login: bool, bare: bool) -> Result<()> {
    let home = ctx.home()?;
    let e = Enabled { folder: s.root.clone(), start: if login { When::Login } else { When::Boot }, domain: ctx.domain_option, env: placement_env() };
    let mut list = services::enabled(home)?;
    let was = match list.iter_mut().find(|o| same_path(&o.folder, &s.root)) {
        Some(had) => Some(std::mem::replace(had, e.clone()).start),
        None => {
            list.push(e.clone());
            None
        }
    };
    write_entry(ctx, home, e.start)?;
    keep(home, &list)?;
    let (scope, when) = (scope_name(s), when_text(e.start));
    match was {
        None => ctx.out.note(&format!("enabled {scope}, its services start {when}")),
        Some(w) if w != e.start => ctx.out.note(&format!("{scope} now starts its services {when}")),
        Some(_) if bare => ctx.out.note(&format!("{scope} was enabled already, its services start {when}")),
        Some(_) => {}
    }
    Ok(())
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let run_args = run::Args { words: Vec::new(), dry_run: args.dry_run, watch: args.watch.clone(), help: false };
    let Some(group) = args.words.first() else {
        let ws = ctx.require_scope()?;
        let s = &ws.model.settings;
        if s.services.is_empty() {
            return Err(failed(format!("no services in {}, `rant enable <group>` adds one", ctx.shown(&s.services_file))));
        }
        if !args.dry_run {
            enable_scope(ctx, s, args.login, true)?;
        }
        return run_services(ctx, &run_args);
    };
    if group == "node" {
        return Err(usage("a service is a group, and `rant new group` makes one for a lone node"));
    }
    let ws = ctx.require_scope()?;
    match ws.named(group) {
        Named::Group => {}
        Named::Node => return Err(usage(format!("`{group}` is a node and a service is a group, `rant new group` makes one for it"))),
        Named::Neither(_) => return Err(failed(format!("no group named `{group}`, see `rant ls groups`"))),
    }
    let params = &args.words[1..];
    ws.plan_group(group, params)?;
    if args.dry_run {
        return run_group(ctx, group, params, &run_args);
    }
    let added = ws.add_service(group, params)?;
    enable_scope(ctx, &ws.model.settings, args.login, false)?;
    let ran = run_group(ctx, group, params, &run_args);
    let (shown, file) = (args.words.join(" "), ctx.shown(&ws.model.settings.services_file));
    ctx.out.note(&if added { format!("added {shown} to the services in {file}") } else { format!("{shown} is a service in {file} already") });
    ran
}
