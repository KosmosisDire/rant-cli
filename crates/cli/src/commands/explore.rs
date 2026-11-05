//! `rant explore`: opens the explorer that `rant install explorer` installs.


use rant_process::Command;

use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::packages;

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(hide = true)]
    pub words: Vec<String>,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if !args.words.is_empty() {
        return Err(usage("explore takes no arguments, `rant run WebExplorer` serves it as a page"));
    }
    let home = ctx.home()?.to_path_buf();
    // the explorer package's node that opens a window
    let node = packages::installed_node(&home, "explorer", "explorer").filter(|n| !n.run.is_empty());
    let Some(node) = node else { return Err(failed("the explorer is not installed, `rant install explorer` installs it")) };
    let mut argv = node.run.clone();
    let domain = ctx.domain()?;
    if domain != 0 {
        argv.extend(["--domain".to_string(), domain.to_string()]);
    }
    // a job name of its own each time, since several explorers may run at once
    let stamp = crate::files::stamp();
    let c = Command { argv: argv.clone(), cwd: node.cwd.clone(), ..Command::default() };
    rant_process::start_child(&c, &home.join("explorer.log"), &format!("Local\\rant-explorer-{stamp}"))?;
    ctx.out.note(&format!("started {}", argv[0]));
    Ok(())
}
