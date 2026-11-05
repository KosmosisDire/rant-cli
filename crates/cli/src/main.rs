//! rant-cli: build, run and inspect Rant nodes.

mod cli;
mod cmake;
mod commands;
mod context;
mod failure;
mod home;
mod names;
mod output;
mod process;
mod scaffold;

use std::io::Write;

use context::Context;
use output::Output;

fn main() {
    // SAFETY: no other thread runs yet. A launcher's node name here was meant for a node,
    // never for rant's own, and nodes rant starts get theirs set.
    unsafe { std::env::remove_var("RANT_NODE_NAME") };
    std::process::exit(run());
}

fn run() -> i32 {
    let cli = match cli::parse(std::env::args_os()) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return e.exit_code();
        }
    };
    if cli.global.version {
        let _ = writeln!(std::io::stdout(), "rant-cli {} (Rant {})", env!("CARGO_PKG_VERSION"), rant::version());
        return 0;
    }
    let Some(command) = cli.command else {
        let _ = writeln!(std::io::stdout(), "{}", cli::command().render_help().to_string().trim_end());
        return 0;
    };
    let mut ctx = Context::new(Output::new(cli.global.format()));
    match commands::run(&mut ctx, command) {
        Ok(()) => 0,
        Err(f) => {
            for m in f.messages() {
                ctx.out.error(&m);
            }
            f.code()
        }
    }
}
