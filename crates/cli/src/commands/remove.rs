//! `rant remove`: installed packages, or the skill rant add gave a library. A linked build folder
//! stays as it is.

use crate::context::Context;
use crate::failure::{Failure, Result, usage};
use crate::packages;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the installed packages, or library skills
    #[arg(value_name = "package")]
    pub names: Vec<String>,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if args.names.is_empty() {
        return Err(usage("remove needs an installed package, `rant ls packages` lists them"));
    }
    let home = ctx.home()?.to_path_buf();
    let mut any_failed = false;
    for name in &args.names {
        match packages::remove(ctx, &home, name) {
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
