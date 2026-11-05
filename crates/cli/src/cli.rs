//! The command line: the global options, every command and its help.

use std::ffi::OsString;

use clap::{Arg, ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::commands::{build, init, new};
use crate::output::Format;

#[derive(Parser, Debug)]
#[command(name = "rant", disable_help_subcommand = true, disable_version_flag = true)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Args, Debug)]
pub struct Global {
    /// answer yes to every question
    #[arg(short, long, global = true)]
    pub yes: bool,
    /// print JSON instead of text
    #[arg(long, global = true)]
    json: bool,
    /// print CSV, a row per record
    #[arg(long, global = true, conflicts_with = "json")]
    csv: bool,
    /// print each record on its own line instead of updating in place
    #[arg(short, long, global = true)]
    lines: bool,
    /// the Rant domain to use and start nodes on, else the workspace's, else RANT_DOMAIN, else 0
    #[arg(long, global = true, value_name = "N")]
    domain: Option<u16>,
    /// show the version
    #[arg(long)]
    pub version: bool,
}

impl Global {
    pub fn format(&self) -> Format {
        match (self.json, self.csv) {
            (true, _) => Format::Json,
            (_, true) => Format::Csv,
            _ => Format::Text,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// make this directory a workspace by writing workspace.rant
    #[command(override_usage = "rant init [dir]")]
    Init(init::Args),
    /// make a workspace or package here or in a folder, or a node or group here
    #[command(override_usage = "rant new workspace|package [folder] | node|group [name]")]
    New(new::Args),
    /// build packages in dependency order
    #[command(override_usage = "rant build [package...]")]
    Build(build::Args),
}

/// The headings of the main help, and the commands under each.
const SECTIONS: &[(&str, &[&str])] = &[("In a workspace", &["init", "new", "build"]), ("On the mesh, anywhere", &[]), ("Setting up", &[])];

const COMMAND_HELP: &str = "{usage-heading} {usage}\n\n{about}\n\n{all-args}";
const MAIN_HELP: &str = "rant: build, run and inspect Rant nodes\n\n{usage-heading} {usage}\n{before-help}Options:\n{options}\n\n\
                         Run `rant <command> --help` for the details of one command.";

pub fn command() -> clap::Command {
    let help = Arg::new("help").short('h').long("help").help("show help, also after a command").action(ArgAction::Help).global(true);
    let cmd = Cli::command().disable_help_flag(true).arg(help).mut_subcommands(|c| c.disable_help_flag(true).help_template(COMMAND_HELP));
    let sections = sections(&cmd);
    cmd.override_usage("rant <command> [args] [options]").help_template(MAIN_HELP).before_help(sections)
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli, clap::Error> {
    let matches = command().try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

/// Each section's commands, one line each: its usage and what it does, lined up.
fn sections(cmd: &clap::Command) -> String {
    let usage = |c: &clap::Command| c.clone().render_usage().to_string().trim_start_matches("Usage: rant ").to_string();
    let mut out = String::new();
    for (heading, names) in SECTIONS {
        let rows: Vec<(String, String)> = names
            .iter()
            .filter_map(|n| cmd.find_subcommand(n))
            .map(|c| (usage(c), c.get_about().map(ToString::to_string).unwrap_or_default()))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let width = rows.iter().map(|(u, _)| u.len()).max().unwrap_or(0);
        out += &format!("\n{heading}:\n");
        for (u, about) in rows {
            out += &format!("  {u:width$}  {about}\n");
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Cli, clap::Error> {
        parse(std::iter::once("rant").chain(words.iter().copied()).map(OsString::from))
    }

    #[test]
    fn every_command_is_in_one_section() {
        let cmd = command();
        for c in cmd.get_subcommands() {
            let count = SECTIONS.iter().filter(|(_, names)| names.contains(&c.get_name())).count();
            assert_eq!(count, 1, "`{}` is in {count} sections", c.get_name());
        }
        clap::Command::debug_assert(cmd);
    }

    #[test]
    fn the_main_help_lists_the_commands_under_their_heading() {
        let help = command().render_help().to_string();
        assert!(help.starts_with("rant: build, run and inspect Rant nodes\n\nUsage: rant <command> [args] [options]\n"), "{help}");
        assert!(help.contains("\nIn a workspace:\n  init [dir]  "), "{help}");
        assert!(help.contains("\nOptions:\n"), "{help}");
    }

    #[test]
    fn a_command_help_starts_with_its_usage() {
        let mut cmd = command();
        let help = cmd.find_subcommand_mut("init").unwrap().render_help().to_string();
        assert!(help.starts_with("Usage: rant init [dir]\n\nmake this directory a workspace"), "{help}");
    }

    #[test]
    fn options_go_before_or_after_the_command_and_words() {
        let cli = parsed(&["--json", "new", "group", "nav"]).unwrap();
        assert_eq!(cli.global.format(), Format::Json);
        let cli = parsed(&["new", "node", "--lang=cpp", "arm", "--csv"]).unwrap();
        assert_eq!(cli.global.format(), Format::Csv);
        let Some(Command::New(a)) = cli.command else { panic!("not new") };
        assert_eq!((a.kind.as_deref(), a.name.as_deref(), a.lang.as_deref()), (Some("node"), Some("arm"), Some("cpp")));
    }

    #[test]
    fn a_double_dash_ends_the_options() {
        let Some(Command::New(a)) = parsed(&["new", "--", "--json"]).unwrap().command else { panic!("not new") };
        assert_eq!((a.kind.as_deref(), a.name.as_deref()), (Some("--json"), None));
    }

    #[test]
    fn mistakes_are_usage_errors() {
        assert!(parsed(&["--nope"]).is_err());
        assert!(parsed(&["new", "--lang"]).is_err());
        assert!(parsed(&["--json", "--csv", "init"]).is_err());
        assert!(parsed(&["init", "a", "b"]).is_err());
        assert_eq!(parsed(&["nothing"]).unwrap_err().exit_code(), 2);
    }
}
