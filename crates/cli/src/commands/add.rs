//! `rant add`: a library added to the package here or in a folder, or moved to another release.
//! Rant is the library when none is named.

use rant_config::paths;

use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::github;
use crate::library::{self, DEFAULT_LIBRARY};
use crate::packages::sources::Sources;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the library, Rant when none is named, then the package folder, here when none is named
    #[arg(value_name = "library")]
    pub words: Vec<String>,
    /// this release of the library, the latest by default
    #[arg(long, value_name = "V")]
    pub version: Option<String>,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let w = &args.words;
    if w.len() > 2 {
        return Err(usage("add takes a library and one folder at most, such as `rant add rant cam`"));
    }
    let mut dir = ctx.cwd()?.to_path_buf();
    if let Some(folder) = w.get(1) {
        dir = dir.join(folder);
        if !dir.is_dir() {
            return Err(failed(format!("no folder `{folder}`")));
        }
        dir = paths::normalize(&dir);
    }
    let home = ctx.home()?.to_path_buf();
    let mut sources = Sources::new(&home)?;
    let lib = library::find(ctx, &mut sources, w.first().map_or(DEFAULT_LIBRARY, String::as_str))?;
    let r = github::release(&lib.repo, args.version.as_deref().unwrap_or_default())?;
    if library::add(ctx, &home, &dir, &lib, &r, args.version.is_some())? { Ok(()) } else { Err(Failure::Reported) }
}
