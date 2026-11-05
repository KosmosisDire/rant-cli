//! Every command, a file each. A command returns its data and the output layer prints it.

pub mod build;
pub mod init;
pub mod new;

use std::path::Path;

use serde::Serialize;

use crate::cli::Command;
use crate::context::Context;
use crate::failure::Result;
use crate::output::{Data, Text};

pub fn run(ctx: &mut Context, command: Command) -> Result<()> {
    match command {
        Command::Init(a) => init::run(ctx, &a).map(|d| ctx.show(&d)),
        Command::New(a) => new::run(ctx, &a).map(|d| ctx.show(&d)),
        Command::Build(a) => build::run(ctx, &a).map(|d| ctx.show(&d)),
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
