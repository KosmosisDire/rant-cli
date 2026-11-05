//! `rant log`: what nodes wrote, every running node's when nothing is named. Several logs
//! interleave, each line led by its node's name.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rant_config::refs::same_name;
use serde::Serialize;

use crate::commands::run::{group_name, in_each, names_group, snapshot, where_running};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::logs::{Follower, logged_nodes, tail};
use crate::output::Style;
use crate::state::RootKind;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// a group, or nodes, after `node` to say so
    #[arg(value_name = "name")]
    pub words: Vec<String>,
    /// keep printing new lines until Ctrl-C
    #[arg(short, long)]
    pub follow: bool,
    /// lines of each log to show first
    #[arg(short = 'n', long, value_name = "N", default_value_t = 20)]
    pub count: usize,
}

/// One line of a log, as --json prints it.
#[derive(Serialize)]
struct Line<'a> {
    node: &'a str,
    line: &'a str,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    ctx.require_scope()?;
    let mut words = args.words.as_slice();
    let node = words.first().is_some_and(|w| w == "node");
    if node {
        words = &words[1..];
        if words.is_empty() {
            return Err(usage("log node takes a node, such as `rant log node talker`"));
        }
    }
    // each name is read where it runs on this machine, as stop finds it, else here
    let mut logs: Vec<(String, PathBuf)> = Vec::new();
    let here = ctx.cwd()?.to_path_buf();
    for w in words {
        let Some(where_) = where_running(ctx, w, node, "log")? else { return Err(Failure::Reported) };
        let mut read = |ctx: &mut Context, _: &str| {
            for n in nodes_of(ctx, w, node)? {
                add(ctx, &mut logs, &n)?;
            }
            Ok(())
        };
        if where_.is_empty() {
            read(ctx, "")?;
        } else {
            in_each(ctx, &where_, read)?;
        }
        ctx.enter(&here);
    }
    if args.words.is_empty() {
        for i in snapshot(ctx)?.instances {
            add(ctx, &mut logs, &i.name)?;
        }
        if logs.is_empty() {
            return Err(failed("nothing rant started is running here, name a node such as `rant log talker`"));
        }
    }

    let width = logs.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
    let several = logs.len() > 1;
    let print = |name: &str, line: &str| {
        let text = if several { format!("{}{line}", ctx.out.paint_out(Style::Dim, &format!("{name:width$} | "))) } else { line.to_string() };
        ctx.out.record(&Line { node: name, line }, &text);
    };
    let mut followers: Vec<Follower> = Vec::new();
    for (name, log) in &logs {
        if args.follow {
            followers.push(Follower::new(log.clone()));
        }
        tail(log, args.count).iter().for_each(|l| print(name, l));
    }
    if !args.follow {
        return Ok(());
    }
    catch_interrupt();
    while !INTERRUPTED.load(Ordering::Relaxed) {
        for ((name, _), f) in logs.iter().zip(&mut followers) {
            f.poll().iter().for_each(|l| print(name, l));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// The log of a node, once, or a failure when it has none.
fn add(ctx: &mut Context, logs: &mut Vec<(String, PathBuf)>, name: &str) -> Result<()> {
    let log = ctx.require_scope()?.model.settings.logs.join(format!("{name}.log"));
    if logs.iter().any(|(_, l)| l == &log) {
        return Ok(());
    }
    if !log.is_file() {
        return Err(failed(format!("`{name}` has no log, see `rant ls nodes`")));
    }
    logs.push((name.to_string(), log));
    Ok(())
}

/// The nodes a name means: a group's nodes, the running ones or else those its plan names,
/// otherwise the node of that name as its log is named.
fn nodes_of(ctx: &mut Context, name: &str, node: bool) -> Result<Vec<String>> {
    if node || !names_group(ctx, name)? {
        let logged = logged_nodes(&ctx.require_scope()?.model.settings.logs);
        let found = logged.iter().find(|n| *n == name).or_else(|| logged.iter().find(|n| same_name(n, name)));
        return Ok(vec![found.map_or(name, |n| n.as_str()).to_string()]);
    }
    let group = group_name(ctx, name)?;
    let st = snapshot(ctx)?;
    let mut out: Vec<String> = Vec::new();
    for r in st.roots.iter().filter(|r| r.kind == RootKind::Group && r.name == group) {
        for i in st.instances.iter().filter(|i| i.roots.contains(&r.key())) {
            if !out.contains(&i.name) {
                out.push(i.name.clone());
            }
        }
    }
    if out.is_empty() {
        out = ctx.require_scope()?.plan_group(name, &[])?.plan.instances.into_iter().map(|i| i.name).collect();
    }
    Ok(out)
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Catches Ctrl-C from here on, so following ends cleanly.
#[cfg(windows)]
fn catch_interrupt() {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
    unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
        let ours = kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT;
        if ours {
            INTERRUPTED.store(true, Ordering::Relaxed);
        }
        ours as i32
    }
    // SAFETY: the handler only sets an atomic.
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
}

#[cfg(unix)]
fn catch_interrupt() {
    extern "C" fn on_signal(_: libc::c_int) {
        INTERRUPTED.store(true, Ordering::Relaxed);
    }
    // SAFETY: the handler only sets an atomic, which is async signal safe.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}
