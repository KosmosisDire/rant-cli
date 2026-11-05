//! rant-cli: build, run and inspect Rant nodes.

mod agents;
mod cli;
mod cmake;
mod commands;
mod context;
mod failure;
mod files;
mod github;
mod home;
mod interrupt;
mod library;
mod logs;
mod mesh;
mod names;
mod net;
mod nodes;
mod output;
mod packages;
mod pick;
mod prompt;
#[cfg(windows)]
mod registry;
mod scaffold;
#[cfg(test)]
mod scratch;
mod services;
mod state;

use std::io::Write;

use context::Context;
use output::Output;

fn main() {
    if let Some(code) = rant_process::helper(std::env::args_os()) {
        std::process::exit(code);
    }
    // SAFETY: no other thread runs yet. A launcher's node name here was meant for a node,
    // never for rant's own, and nodes rant starts get theirs set.
    unsafe { std::env::remove_var("RANT_NODE_NAME") };
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "__services") {
        return serve(&args[2..]);
    }
    let cli = match cli::parse(args) {
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
    let mut ctx = Context::new(Output::new(cli.global.format(), cli.global.lines), cli.global.yes, cli.global.domain);
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

/// The service process, `rant __services`, and as the boot and login entries start it with the
/// home it serves, `rant __services --boot HOME` or `--login HOME`.
fn serve(args: &[std::ffi::OsString]) -> i32 {
    let out = Output::new(output::Format::Text, false);
    let when = match args.first().and_then(|a| a.to_str()) {
        None => None,
        Some("--boot") if args.len() == 2 => Some(services::When::Boot),
        Some("--login") if args.len() == 2 => Some(services::When::Login),
        _ => {
            out.error("`rant __services` is rant's own, never typed");
            return 2;
        }
    };
    if when.is_some() {
        // SAFETY: no other thread runs yet, and the nodes it starts take this home too.
        unsafe { std::env::set_var("RANT_HOME", &args[1]) };
    }
    match home::rant_home() {
        Some(h) => services::serve(&h, when),
        None => {
            out.error("rant has no home folder, set HOME or RANT_HOME");
            1
        }
    }
}
