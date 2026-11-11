//! `rant disable`: the services stop and no longer start at boot or login, or one group leaves
//! them.

use rant_config::refs::same_name;
use rant_config::{ServiceCheck, Workspace};

use crate::commands::enable::{keep, scope_at, scope_name, service_key, when_text};
use crate::commands::run::group_name;
use crate::commands::stop::stop_roots;
use crate::context::Context;
use crate::files::same_path;
use crate::failure::{Result, failed, usage};
use crate::nodes::Session;
use crate::services::{self, Enabled};
use crate::state::Root;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group with its key=value params, taken off the services
    #[arg(value_name = "group")]
    pub words: Vec<String>,
}

/// Stops the runs of these services of the scope that run, all in one block.
fn stop_services(ctx: &mut Context, ws: &Workspace, which: &[usize]) -> Result<()> {
    let home = ctx.home()?.to_path_buf();
    let mut s = Session::open(&home, &ws.model.settings.data)?;
    let checks = ws.services();
    let mut roots: Vec<Root> = Vec::new();
    for &i in which {
        let Ok(c) = &checks[i].checked else { continue };
        let root = Root::planned(&c.full, &c.values);
        if s.state().root(&root.key()).is_some() && !roots.contains(&root) {
            roots.push(root);
        }
    }
    if roots.is_empty() {
        return Ok(());
    }
    stop_roots(ctx, &home, s, &roots)
}

/// With no words, stops the scope's services and drops its start, keeping their blocks. With a
/// group, removes its blocks, or the one its params pick, and stops those runs.
pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let ws = ctx.require_scope()?;
    let home = ctx.home()?.to_path_buf();
    let s = &ws.model.settings;
    let Some(group) = args.words.first() else {
        let (mine, kept): (Vec<Enabled>, Vec<Enabled>) = services::enabled(&home)?.into_iter().partition(|o| same_path(&o.folder, &s.root));
        let Some(had) = mine.first() else { return Err(failed(format!("{} is not enabled, see `rant ls services`", scope_name(s)))) };
        keep(&home, &kept)?;
        let stopped = stop_services(ctx, &ws, &(0..s.services.len()).collect::<Vec<_>>());
        ctx.out.note(&format!("disabled {}, its services no longer start {}", scope_name(s), when_text(had.start)));
        return stopped;
    };
    if group == "node" {
        return Err(usage("a service is a group, such as `rant disable Bridge`"));
    }
    let full = group_name(ctx, group)?;
    let params = &args.words[1..];
    let key = if params.is_empty() {
        None
    } else {
        let gp = ws.plan_group(group, params)?;
        Some(Root::planned(&gp.group, &gp.values).key())
    };
    let checks = ws.services();
    let of_group = |c: &ServiceCheck| match &c.checked {
        Ok(k) => k.full == full,
        Err(_) => same_name(&c.service.group, group),
    };
    let gone: Vec<usize> = (0..checks.len()).filter(|&i| of_group(&checks[i]) && key.as_ref().is_none_or(|k| service_key(&checks[i]).as_ref() == Ok(k))).collect();
    if gone.is_empty() {
        for o in services::enabled(&home)?.iter().filter(|o| !same_path(&o.folder, &s.root)) {
            if scope_at(&home, &o.folder).is_ok_and(|other| other.model.settings.services.iter().any(|sv| same_name(&sv.group, group))) {
                return Err(failed(format!("`{group}` is a service of {}, disable it there", ctx.shown(&o.folder))));
            }
        }
        return Err(failed(format!("`{}` is not a service here, see `rant ls services`", args.words.join(" "))));
    }
    ws.remove_services(&gone)?;
    let stopped = stop_services(ctx, &ws, &gone);
    for &i in &gone {
        ctx.out.note(&format!("removed {} from the services in {}", checks[i].shown(), ctx.shown(&s.services_file)));
    }
    stopped
}
