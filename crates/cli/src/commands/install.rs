//! `rant install`: packages by name from the sources, from a repo by its link, or from a local
//! build folder or archive with --from. None named on a terminal is a checklist.

use std::io::IsTerminal;

use rant_config::database;
use rant_config::home::{self, AgentSkills};

use crate::agents;
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::github;
use crate::packages::sources::{Offer, Sources, carried, from_repo};
use crate::packages::vsix::{self, Wanted};
use crate::packages::{self, Source, skills};
use crate::pick::{self, Choice};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// packages by name, or GitHub repos as owner/name or a link
    #[arg(value_name = "package|repo")]
    pub words: Vec<String>,
    /// with one package: this release instead of the newest
    #[arg(long, value_name = "V")]
    pub version: Option<String>,
    /// with one package: a local build folder, linked, or a .tar.gz archive, instead of its source
    #[arg(long, value_name = "PATH")]
    pub from: Option<String>,
    /// for a VS Code extension: the editor's command line program, default `code`, such as `cursor`
    #[arg(long, value_name = "PROGRAM")]
    pub editor: Option<String>,
    /// for a VS Code extension: the editor profile, default the one this VS Code terminal's folder opens in, else asked
    #[arg(long, value_name = "NAME")]
    pub profile: Option<String>,
}

impl Args {
    fn wanted(&self) -> Wanted {
        Wanted { editor: self.editor.clone(), profile: self.profile.clone() }
    }
}

/// The source a word names: a package through the sources, a repo by its link, or --from.
fn source_of(ctx: &Context, args: &Args, sources: &mut Option<Sources>, word: &str) -> Result<Source> {
    let repo = if database::is_name(word) { None } else { database::repo_of(word) };
    if !database::is_name(word) && repo.is_none() {
        return Err(usage(format!("`{word}` is no package name, which is lowercase letters, digits and _, and no GitHub repo")));
    }
    if let Some(from) = &args.from {
        if repo.is_some() {
            return Err(usage("--from takes the name of the package it holds, not a repo"));
        }
        return Ok(Source { name: word.into(), repo: String::new(), path: ctx.cwd()?.join(from), named_by: "--from".into(), library: None });
    }
    if let Some(r) = repo {
        return from_repo(&r);
    }
    if sources.is_none() {
        *sources = Some(Sources::new(ctx.home()?)?);
    }
    let sources = sources.as_mut().expect("just made");
    match sources.find(word)? {
        Some(s) => Ok(s),
        None => {
            sources.problems().iter().for_each(|p| ctx.out.warn(p));
            Err(failed(format!("no package `{word}` in config.rant or the package databases, `rant ls packages --available` lists them")))
        }
    }
}

/// Each package named. With several, one that fails leaves the rest to go on. Returns the names
/// that installed, and whether one failed.
fn install_all(ctx: &Context, args: &Args, words: &[String]) -> Result<(Vec<String>, bool)> {
    if (args.version.is_some() || args.from.is_some()) && words.len() > 1 {
        return Err(usage(format!("{} takes one package", if args.from.is_some() { "--from" } else { "--version" })));
    }
    if args.version.is_some() && args.from.is_some() {
        return Err(usage("--version picks a release, and --from has none"));
    }
    let (home, wanted) = (ctx.home()?.to_path_buf(), args.wanted());
    let mut sources = None;
    let (mut done, mut any_failed) = (Vec::new(), false);
    for word in words {
        let one = source_of(ctx, args, &mut sources, word).and_then(|s| {
            if args.version.is_some() && s.repo.is_empty() {
                return Err(usage(format!("--version picks a release, and {word} comes from {} in config.rant", rant_config::paths::slash(&s.path))));
            }
            packages::install(ctx, &home, &s, args.version.as_deref().unwrap_or_default(), &wanted).map(|_| s.name)
        });
        match one {
            Ok(name) => done.push(name),
            Err(e) if words.len() == 1 => return Err(e),
            Err(e) => {
                ctx.out.failure(&e);
                any_failed = true;
            }
        }
    }
    Ok((done, any_failed))
}

/// Whether each offer is a VS Code extension its latest release carries, all false when that
/// cannot be told.
fn extensions(ctx: &Context, offers: &[Offer]) -> Vec<bool> {
    let mut out = vec![false; offers.len()];
    let at: Vec<usize> = (0..offers.len()).filter(|&i| !offers[i].source.repo.is_empty() && offers[i].record.is_none()).collect();
    match github::latest(&at.iter().map(|&i| offers[i].source.repo.clone()).collect::<Vec<_>>()) {
        Ok(found) => {
            for (&i, r) in at.iter().zip(found) {
                out[i] = r.is_some_and(|r| carried(&r, &offers[i].source.name, github::platform()).extension.is_some());
            }
        }
        Err(e) => ctx.out.warn(&e.messages().join(", ")),
    }
    out
}

/// The packages the checklist ticks while they are not installed.
const TICKED: [&str; 2] = ["explorer", "vscode"];

/// The checklist's row for the CLI's own skill, ticked while an agent is found that has it not.
fn cli_skill() -> Choice {
    let (found, given) = (!agents::detected().is_empty(), !skills::cli_links().is_empty());
    let state = if given { "installed, " } else if found { "" } else { "no AI coding agent found, " };
    Choice { label: "Rant Cli Skill".into(), note: format!("{state}teaches AI coding agents to use rant-cli"), checked: found && !given }
}

/// The checklist's last row, the agent_skills setting of config.rant, ticked unless it says ask.
fn auto_skills(setting: Option<AgentSkills>) -> Choice {
    let note = "gives the skill a package or library carries to AI coding agents without asking";
    Choice { label: "Auto install agent skills".into(), note: note.into(), checked: setting != Some(AgentSkills::Ask) }
}

/// With no names on a terminal: a checklist of the CLI's skill, every package there is and the
/// auto skills setting, those of TICKED not installed ticked but an editor extension with no
/// editor found.
fn pick(ctx: &mut Context, args: &Args) -> Result<()> {
    if ctx.yes || !std::io::stdin().is_terminal() || !ctx.out.terminal_err() || args.version.is_some() || args.from.is_some() {
        return Err(usage("install needs a package, `rant ls packages --available` lists them"));
    }
    ctx.out.note("looking up the packages");
    let mut sources = Sources::new(ctx.home()?)?;
    let offers: Vec<Offer> = sources.offers("")?.into_iter().filter(|o| o.source.library.is_none()).collect();
    sources.problems().iter().for_each(|p| ctx.out.warn(p));
    let skipped = if vsix::editor_found(ctx, &args.wanted()) { vec![false; offers.len()] } else { extensions(ctx, &offers) };
    let packages = offers
        .iter()
        .zip(&skipped)
        .map(|(o, &skip)| {
            let state = match &o.record {
                Some(r) if r.version.is_empty() => "installed, ".to_string(),
                Some(r) => format!("installed {}, ", r.version),
                None if skip => "no editor found, ".to_string(),
                None => String::new(),
            };
            Choice { label: o.source.name.clone(), note: state + &o.description, checked: o.record.is_none() && !skip && TICKED.contains(&o.source.name.as_str()) }
        });
    let home = ctx.home()?.to_path_buf();
    let setting = home::read(&home)?.agent_skills;
    let choices = std::iter::once(cli_skill()).chain(packages).chain([auto_skills(setting)]).collect();
    // the installers read the exit code to tell a cancel from a confirm
    let Some(ticked) = pick::pick(&ctx.out, "To install", "Install", choices) else { return Err(Failure::Cancelled) };
    let auto = if ticked.contains(&(offers.len() + 1)) { AgentSkills::Auto } else { AgentSkills::Ask };
    if setting != Some(auto) {
        home::set_agent_skills(&home, auto)?;
        let word = if auto == AgentSkills::Auto { "auto" } else { "ask" };
        ctx.out.note(&format!("set agent_skills = \"{word}\" in {}", rant_config::paths::slash(&home::file(&home))));
    }
    let mut skill_failed = false;
    if ticked.first() == Some(&0)
        && let Err(e) = skills::give_cli(ctx, &home)
    {
        ctx.out.failure(&e);
        skill_failed = true;
    }
    let names: Vec<String> = ticked.iter().filter(|&&i| i > 0 && i <= offers.len()).map(|&i| offers[i - 1].source.name.clone()).collect();
    let (_, any_failed) = install_all(ctx, args, &names)?;
    if any_failed || skill_failed { Err(Failure::Reported) } else { Ok(()) }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if args.words.is_empty() {
        return pick(ctx, args);
    }
    let (_, any_failed) = install_all(ctx, args, &args.words)?;
    if any_failed { Err(Failure::Reported) } else { Ok(()) }
}
