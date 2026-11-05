//! Every command, a file each. A command returns its data and the output layer prints it.

pub mod add;
pub mod build;
pub mod call;
pub mod calls;
pub mod disable;
pub mod enable;
pub mod explore;
pub mod info;
pub mod init;
pub mod install;
pub mod kinds;
pub mod log;
pub mod ls;
pub mod new;
pub mod play;
pub mod publish;
pub mod record;
pub mod remove;
pub mod restart;
pub mod run;
pub mod setup;
pub mod stop;
pub mod stream;
pub mod sub;
pub mod summary;
pub mod uninstall;
pub mod update;

use std::path::Path;

use serde::Serialize;

use crate::cli::Command;
use crate::context::Context;
use crate::failure::Result;
use crate::output::{Data, Text};

pub fn run(ctx: &mut Context, command: Command) -> Result<()> {
    match command {
        Command::Init(a) => init::run(ctx, &a).map(|d| ctx.show(&d)),
        Command::New(a) => new::run(ctx, &a),
        Command::Build(a) => build::run(ctx, &a).map(|d| ctx.show(&d)),
        // these print as they go
        Command::Run(a) => run::run(ctx, &a),
        Command::Stop(a) => stop::run(ctx, &a),
        Command::Restart(a) => restart::run(ctx, &a),
        Command::Enable(a) => enable::run(ctx, &a),
        Command::Disable(a) => disable::run(ctx, &a),
        Command::Log(a) => log::run(ctx, &a),
        Command::Ls(a) => ls::run(ctx, &a),
        Command::Info(a) => info::run(ctx, &a),
        Command::Sub(a) => sub::run(ctx, &a),
        Command::Pub(a) => publish::run(ctx, &a),
        Command::Call(a) => call::run(ctx, &a),
        Command::Record(a) => record::run(ctx, &a),
        Command::Play(a) => play::run(ctx, &a),
        Command::Explore(a) => explore::run(ctx, &a),
        Command::Install(a) => install::run(ctx, &a),
        Command::Remove(a) => remove::run(ctx, &a),
        Command::Update(a) => update::run(ctx, &a),
        Command::Add(a) => add::run(ctx, &a),
        Command::Setup(a) => setup::run(ctx, &a),
        Command::Uninstall(a) => uninstall::run(ctx, &a),
    }
}

/// The files a command made, and the build file a node it made joined.
#[derive(Debug, Serialize)]
pub struct Made {
    pub created: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<Added>,
}

#[derive(Debug, Serialize)]
pub struct Added {
    pub node: String,
    pub to: String,
}

impl Data for Made {
    fn text(&self, t: &Text) -> String {
        let mut lines: Vec<String> = self.created.iter().map(|f| format!("created {}", t.path(Path::new(f)))).collect();
        if let Some(a) = &self.added {
            lines.push(format!("added {} to {}", a.node, t.path(Path::new(&a.to))));
        }
        lines.join("\n")
    }
}
