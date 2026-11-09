//! `rant setup`: tab completion for the shells found here, or the ones named, and the CLI's
//! skill for the AI coding agents found when `skill` is named.

use serde::Serialize;

use crate::agents;
use crate::complete::shells::{self, SHELLS, Shell};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::output::{Data, Text};
use crate::packages::skills;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// shells by name, every one found here when none is, or skill for rant-cli's skill
    #[arg(value_name = "shell|skill")]
    pub shells: Vec<String>,
    /// print the hook for one shell instead of installing it
    #[arg(long)]
    pub print: bool,
}

fn shell_names() -> String {
    SHELLS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")
}

fn named(name: &str) -> Result<&'static Shell> {
    shells::find(name).ok_or_else(|| usage(format!("unknown shell `{name}`, rant knows {}", shell_names())))
}

/// A shell set up, and where its hook loads from.
#[derive(Serialize)]
struct Hooked {
    shell: &'static str,
    loads_from: String,
}

#[derive(Serialize)]
#[serde(transparent)]
struct Hooks(Vec<Hooked>);

impl Data for Hooks {
    fn text(&self, _: &Text) -> String {
        self.0.iter().map(|h| format!("{}: tab completion loads from {}", h.shell, h.loads_from)).collect::<Vec<_>>().join("\n")
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if args.print {
        let [one] = args.shells.as_slice() else { return Err(usage(format!("--print takes one shell, one of {}", shell_names()))) };
        ctx.out.line(named(one)?.hook.trim_end());
        return Ok(());
    }
    let home = ctx.home()?.to_path_buf();
    let skill = args.shells.iter().any(|w| w == "skill");
    let words: Vec<&String> = args.shells.iter().filter(|w| *w != "skill").collect();
    if skill {
        skills::give_cli(ctx, &home)?;
        if words.is_empty() {
            return Ok(());
        }
    }
    let chosen = if words.is_empty() { shells::detected() } else { words.into_iter().map(|n| named(n)).collect::<Result<_>>()? };
    if chosen.is_empty() {
        return Err(failed(format!("found no shell to set up, name one of {}", shell_names())));
    }
    let (mut done, mut any_failed) = (Vec::new(), false);
    for sh in chosen {
        match shells::install(sh, &home) {
            Ok(loads_from) => done.push(Hooked { shell: sh.name, loads_from }),
            Err(e) => {
                ctx.out.warn(&format!("{}: {}", sh.name, e.messages().join(", ")));
                any_failed = true;
            }
        }
    }
    ctx.show(&Hooks(done));
    ctx.out.note("new shells load it, this one does not");
    let names: Vec<&str> = agents::detected().iter().map(|a| a.name).collect();
    if !skill && !names.is_empty() && skills::cli_links().is_empty() {
        ctx.out.note(&format!("`rant setup skill` teaches {} to use rant-cli", names.join(", ")));
    }
    if any_failed { Err(Failure::Reported) } else { Ok(()) }
}
