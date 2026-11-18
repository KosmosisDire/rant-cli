//! `rant remove`: installed packages, or a library's copies on this machine and the skill rant gave
//! it. A linked build folder stays as it is.

use std::path::Path;

use crate::context::Context;
use crate::failure::{Failure, Result, usage};
use crate::library;
use crate::packages;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the installed packages or libraries
    #[arg(value_name = "package")]
    pub names: Vec<String>,
}

/// A library rant knows by name, its copies and skill, else an installed package. A name is never both.
fn remove(ctx: &Context, home: &Path, name: &str) -> Result<()> {
    if let Some(lib) = library::known(home).into_iter().find(|l| l.name == name)
        && library::uninstall(ctx, home, &lib)?
    {
        return Ok(());
    }
    packages::remove(ctx, home, name)
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if args.names.is_empty() {
        return Err(usage("remove needs an installed package, `rant ls packages` lists them"));
    }
    let home = ctx.home()?.to_path_buf();
    let mut any_failed = false;
    for name in &args.names {
        match remove(ctx, &home, name) {
            Ok(()) => {}
            Err(e) if args.names.len() == 1 => return Err(e),
            Err(e) => {
                ctx.out.failure(&e);
                any_failed = true;
            }
        }
    }
    if any_failed { Err(Failure::Reported) } else { Ok(()) }
}
