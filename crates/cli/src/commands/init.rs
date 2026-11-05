use std::path::PathBuf;

use rant_config::{paths, settings};

use super::Made;
use crate::context::Context;
use crate::failure::Result;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// the folder to make a workspace, this one by default
    #[arg(value_name = "dir")]
    pub dir: Option<PathBuf>,
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<Made> {
    let cwd = ctx.cwd()?;
    let dir = args.dir.as_ref().map_or_else(|| cwd.to_path_buf(), |d| cwd.join(d));
    let root = settings::init(&dir)?;
    Ok(Made { created: vec![paths::slash(&paths::normalize(&root.join(settings::WORKSPACE_FILE)))], added: None })
}
