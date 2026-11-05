//! `rant update`: rant-cli and the installed packages, then the libraries the workspace here
//! uses, each after its own question. Named ones alone go without the rest.

use std::path::Path;

use rant_process::Command;

use crate::context::Context;
use crate::failure::{Failure, Result, failed};
use crate::github::{self, Asset};
use crate::library::{self, Library};
use crate::nodes;
use crate::output::spoken;
use crate::packages::sources::Sources;
use crate::packages::vsix::Wanted;
use crate::packages::{self, Update, skills};

const CLI_REPO: &str = "KosmosisDire/rant-cli";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(clap::Args, Debug)]
pub struct Args {
    /// only these packages and libraries
    #[arg(value_name = "package|library")]
    pub words: Vec<String>,
}

/// A package's move as the question lists it: explorer 0.0.19 to 0.0.20, from its source.
fn shown(u: &Update) -> String {
    let from = if u.source.repo.is_empty() { rant_config::paths::slash(&u.source.path) } else { u.source.repo.clone() };
    let had = if u.had.is_empty() { String::new() } else { format!(" {}", u.had) };
    let to = if u.to.is_empty() { String::new() } else { format!(" to {}", u.to) };
    format!("{}{had}{to}, from {from}", u.source.name)
}

/// Replaces this binary with its newest release and moves each installed package, or those named,
/// to what its source offers, after one question and never back. False when one failed.
fn update_programs(ctx: &Context, home: &Path, sources: &mut Sources, named: &[String]) -> Result<bool> {
    let all = named.is_empty();
    let mut current: Vec<String> = Vec::new();
    let mut cli: Option<(String, Asset)> = None;
    if all {
        let r = github::release(CLI_REPO, "")?;
        if github::newer(&r.version, VERSION) {
            let asset = format!("rant-{}-{}{}", r.version, github::platform(), std::env::consts::EXE_SUFFIX);
            let a = r.asset(&asset).ok_or_else(|| failed(format!("the rant-cli {} release has no {asset}", r.version)))?;
            cli = Some((r.version.clone(), a.clone()));
        } else {
            current.push(format!("rant-cli {VERSION}"));
        }
    }
    // a library's skill is installed as a package is, and moves with the packages
    let mut moved_skills = Vec::new();
    if all {
        let had = skills::skills(home);
        let latest = github::latest(&had.iter().map(|s| s.repo.clone()).collect::<Vec<_>>())?;
        for (s, r) in had.into_iter().zip(latest) {
            match r.filter(|r| github::newer(&r.version, &s.version)) {
                Some(r) => moved_skills.push((s, r)),
                None => current.push(format!("the skill of {} {}", s.name, s.version)),
            }
        }
    }
    let mut ok = true;
    let mut moves: Vec<Update> = Vec::new();
    let names = if all { packages::installed(home) } else { named.to_vec() };
    for name in &names {
        match packages::check(ctx, home, sources, name) {
            Ok(Some(u)) if u.needed => moves.push(u),
            Ok(Some(u)) => current.push(if u.had.is_empty() { name.clone() } else { format!("{name} {}", u.had) }),
            Ok(None) => {}
            Err(e) => {
                ctx.out.failure(&e);
                ok = false;
            }
        }
    }
    if cli.is_none() && moves.is_empty() && moved_skills.is_empty() {
        if !current.is_empty() {
            ctx.out.line(&format!("{} {} up to date", spoken(&current), if current.len() > 1 { "are" } else { "is" }));
        }
        return Ok(ok);
    }

    let me = crate::home::program()?;
    ctx.out.line("This updates:");
    if let Some((v, _)) = &cli {
        ctx.out.line(&format!("  rant-cli {VERSION} to {v}, over {}", rant_config::paths::slash(&me)));
    }
    for m in &moves {
        ctx.out.line(&format!("  {}", shown(m)));
    }
    for (s, r) in &moved_skills {
        ctx.out.line(&format!("  the skill of {} {} to {}, from {}", s.name, s.version, r.version, s.repo));
    }
    if !ctx.confirm("Update?") {
        return Ok(false);
    }
    for (s, r) in &moved_skills {
        if let Err(e) = skills::give(ctx, home, &s.name, &s.repo, r, false) {
            ctx.out.failure(&e);
            ok = false;
        }
    }
    for m in &moves {
        // Windows will not replace a folder a node runs from, so its nodes stop meanwhile
        let folder = packages::dir(home).join(&m.source.name);
        let paused = if packages::is_link(&folder) { Vec::new() } else { nodes::pause_from(home, &folder)? };
        let stopped = spoken(&paused.iter().map(|p| p.rec.name.as_str()).collect::<Vec<_>>());
        if !paused.is_empty() {
            ctx.out.note(&format!("stopped {stopped} while {} changes", m.source.name));
        }
        let version = if m.source.repo.is_empty() { "" } else { m.to.as_str() };
        if let Err(e) = packages::install(ctx, home, &m.source, version, &Wanted::default()) {
            ctx.out.failure(&e);
            ok = false;
        }
        if paused.is_empty() {
            continue;
        }
        let failures = nodes::resume(home, &paused);
        failures.iter().for_each(|f| ctx.out.warn(&format!("not started again, {f}")));
        if failures.is_empty() {
            ctx.out.note(&format!("started {stopped} again"));
        } else {
            ok = false;
        }
    }
    let Some((v, a)) = cli else { return Ok(ok) };
    github::download(&a.url, &me, true)?;
    ctx.out.line(&format!("updated to rant-cli {v}"));
    // the new binary holds new shell hooks, written again where setup wrote them before
    let hooks = home.join("completions");
    for (file, shell) in [("rant.bash", "bash"), ("rant.zsh", "zsh"), ("rant.ps1", "powershell")] {
        let setup = Command::new([me.to_string_lossy().into_owned(), "setup".into(), shell.into()], std::env::temp_dir());
        if hooks.join(file).exists() && rant_process::run(&setup) != Ok(0) {
            ctx.out.warn(&format!("tab completion for {shell} was not updated, run `rant setup {shell}`"));
        }
    }
    Ok(ok)
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    let home = ctx.home()?.to_path_buf();
    let mut sources = Sources::new(&home)?;
    let (mut programs, mut libs): (Vec<String>, Vec<Library>) = (Vec::new(), Vec::new());
    for word in &args.words {
        let s = if packages::record(&home, word).is_some() { None } else { sources.find(word)? };
        if s.is_some_and(|s| s.library.is_some()) {
            libs.push(library::find(ctx, &mut sources, word)?);
        } else {
            programs.push(word.clone());
        }
    }
    let all = args.words.is_empty();
    let mut ok = true;
    if all || !programs.is_empty() {
        ok = update_programs(ctx, &home, &mut sources, &programs)?;
    }
    if !all && libs.is_empty() {
        return if ok { Ok(()) } else { Err(Failure::Reported) };
    }
    let Some(root) = ctx.workspace().map(|ws| ws.root.clone()) else {
        if !all {
            return Err(failed("rant update moves a library through a workspace, and none is here"));
        }
        return if ok { Ok(()) } else { Err(Failure::Reported) };
    };
    if all {
        libs = library::all(&mut sources)?;
    }
    let uses = library::under(&root, &libs);
    if uses.is_empty() {
        if !all {
            let others = if libs.len() > 1 { " or the others named" } else { "" };
            ctx.out.note(&format!("nothing in this workspace uses {}{others}", libs[0].name));
        }
        return if ok { Ok(()) } else { Err(Failure::Reported) };
    }
    if all || !programs.is_empty() {
        ctx.out.line("");
    }
    ok &= library::update(ctx, &home, &uses, &libs)?;
    if ok { Ok(()) } else { Err(Failure::Reported) }
}
