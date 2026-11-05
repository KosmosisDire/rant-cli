//! `rant uninstall`: rant-cli, the packages it installed and what setup added, never a
//! workspace. The bridge config stays, since it holds the user's passphrase.

use std::path::{Path, PathBuf};

use rant_process::Command;
use serde::Serialize;

use crate::complete::shells::{self, Added};
use crate::context::Context;
use crate::failure::{Failure, Result, failed, usage};
use crate::library;
use crate::output::{Data, Text};
use crate::packages::{self, is_link, skills, unlink, vsix};
use crate::services::{When, entry};

#[derive(clap::Args, Debug)]
pub struct Args {
    #[arg(hide = true)]
    pub words: Vec<String>,
}

fn bridge_config(home: &Path) -> PathBuf {
    home.join("bridge.yaml")
}

/// What the home holds but the bridge config.
fn removable(home: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(home).into_iter().flatten().flatten().map(|e| e.path());
    entries.filter(|p| *p != bridge_config(home)).collect()
}

/// The home and all in it but the bridge config: this rant, the packages, the shell hooks,
/// caches, the workspace list and the NuGet feed.
fn remove_home(home: &Path) -> Result<()> {
    packages::unlink_all(home);
    for p in removable(home) {
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    if removable(home).is_empty() {
        // stays when the bridge config is in it
        let _ = std::fs::remove_dir(home);
        return Ok(());
    }
    if cfg!(windows) { remove_later(home) } else { Err(failed(format!("cannot remove all of {}", home.display()))) }
}

/// What remove_home could not delete, a running program such as this rant or an open explorer,
/// goes once this process has ended, by a script that waits for it a minute.
fn remove_later(home: &Path) -> Result<()> {
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    let mut todo = vec![home.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for p in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| *p != bridge_config(home)) {
            if p.is_dir() {
                todo.push(p.clone());
                dirs.push(p);
            } else {
                files.push(p);
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.as_os_str().len()));
    dirs.push(home.to_path_buf());
    let q = |p: &PathBuf| format!("\"{}\"", p.display());
    let mut s = String::from("@echo off\r\nchcp 65001 >nul\r\nset n=0\r\n:again\r\n");
    files.iter().for_each(|f| s += &format!("del /f /q {} 2>nul\r\n", q(f)));
    files.iter().for_each(|f| s += &format!("if exist {} goto wait\r\n", q(f)));
    s += "goto dirs\r\n:wait\r\nset /a n+=1\r\nif %n% geq 60 goto dirs\r\nping -n 2 127.0.0.1 >nul\r\ngoto again\r\n:dirs\r\n";
    dirs.iter().for_each(|d| s += &format!("rmdir {} 2>nul\r\n", q(d)));
    s += "(goto) 2>nul & del \"%~f0\"\r\n";
    let script = std::env::temp_dir().join(format!("rant-uninstall-{}.cmd", std::process::id()));
    std::fs::write(&script, s).map_err(|e| failed(format!("cannot write {}: {e}", script.display())))?;
    let c = Command::new(["cmd.exe".to_string(), "/d".into(), "/c".into(), script.display().to_string()], std::env::temp_dir());
    rant_process::start_detached(&c, Path::new("NUL"), &format!("Local\\rant-uninstall-{}", crate::files::stamp()))?;
    Ok(())
}

/// The bin folder on the user PATH, where the install script puts it.
#[cfg(windows)]
fn user_path(steps: &mut Vec<Added>, bin: PathBuf) {
    use crate::registry;
    let Some((value, ty)) = registry::read("Environment", "Path") else { return };
    let mine = |e: &&str| crate::files::same_path(Path::new(e), &bin);
    let entries: Vec<&str> = value.split(';').filter(|e| !e.is_empty()).collect();
    if !entries.iter().any(mine) {
        return;
    }
    let kept = entries.iter().filter(|e| !mine(e)).copied().collect::<Vec<_>>().join(";");
    let remove = move || {
        use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
        if !registry::write("Environment", "Path", &kept, ty) {
            return Err(failed("cannot change your user PATH"));
        }
        // open programs such as Explorer read the change from this message
        let what: Vec<u16> = "Environment".encode_utf16().chain(Some(0)).collect();
        // SAFETY: the text outlives the call, which waits at most 2 s.
        unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, std::ptr::null_mut()) };
        Ok(())
    };
    steps.push(Added { path: bin, part: "on your user PATH".into(), remove: Box::new(remove) });
}

fn step(path: PathBuf, part: String, remove: impl Fn() -> Result<()> + 'static) -> Added {
    Added { path, part, remove: Box::new(remove) }
}

/// Every link a skill made, each undone on its own.
fn links(steps: &mut Vec<Added>, links: &[PathBuf], of: &str) {
    for l in links.iter().filter(|l| is_link(l)) {
        let link = l.clone();
        steps.push(step(l.clone(), format!("the skill of {of}"), move || unlink(&link)));
    }
}

/// Where a step is, with the separators of this OS, and what part of it.
fn shown(s: &Added) -> String {
    let at = s.path.components().collect::<PathBuf>().display().to_string();
    match (s.path.as_os_str().is_empty(), s.part.is_empty()) {
        (true, _) => s.part.clone(),
        (false, true) => at,
        (false, false) => format!("{at} ({})", s.part),
    }
}

/// What uninstall removed, and what it could not.
#[derive(Serialize)]
struct Uninstalled {
    removed: Vec<String>,
    failed: Vec<String>,
}

impl Data for Uninstalled {
    fn text(&self, _: &Text) -> String {
        match (self.removed.is_empty(), self.failed.is_empty()) {
            (true, true) => "rant-cli left nothing here to remove".into(),
            (_, true) => "uninstalled rant-cli".into(),
            _ => "uninstalled rant-cli, except what the warnings name".into(),
        }
    }
}

pub fn run(ctx: &mut Context, args: &Args) -> Result<()> {
    if !args.words.is_empty() {
        return Err(usage("uninstall takes no arguments"));
    }
    let home = ctx.home()?.to_path_buf();
    // RANT_HOME may point anywhere, and only a folder of rant's own is deleted whole
    if home.file_name().is_none_or(|n| n != ".rant") {
        return Err(failed(format!("RANT_HOME is {}, uninstall only removes a folder named .rant", home.display())));
    }
    // the folders the installers changed, then what setup added, then the home itself
    let mut steps = shells::added(&home);
    #[cfg(windows)]
    user_path(&mut steps, home.join("bin"));
    if library::nuget_feed_added(&home) {
        steps.push(step(PathBuf::new(), "the NuGet source rant".into(), || {
            let dotnet = Command::new(["dotnet", "nuget", "remove", "source", "rant"], std::env::temp_dir());
            if rant_process::find_program("dotnet", &dotnet.cwd).is_none() {
                return Err(failed("dotnet not found, remove the NuGet source rant by hand"));
            }
            if rant_process::run(&dotnet)? != 0 {
                return Err(failed("dotnet could not remove the NuGet source rant"));
            }
            Ok(())
        }));
    }
    for name in packages::installed(&home) {
        let Some(r) = packages::record(&home, &name) else { continue };
        links(&mut steps, &r.skills, &name);
        if r.extension.is_empty() || !r.editor.is_file() {
            continue;
        }
        let t = vsix::Target { editor: r.editor, profile: r.profile };
        if !vsix::installed_version(&t, &r.extension).is_empty() {
            let part = format!("the VS Code extension {}, through {}", r.extension, vsix::shown(&t));
            steps.push(step(PathBuf::new(), part, move || vsix::remove(&t, &r.extension)));
        }
    }
    for s in skills::skills(&home) {
        links(&mut steps, &s.links, &s.name);
    }
    if entry::present(When::Login, &home) {
        let h = home.clone();
        steps.push(step(PathBuf::new(), "the login entry that starts what rant enable named".into(), move || entry::remove(When::Login, &h)));
    }
    if entry::present(When::Boot, &home) {
        let h = home.clone();
        steps.push(step(PathBuf::new(), format!("{}, which asks for admin", entry::boot_place()), move || entry::remove(When::Boot, &h)));
    }
    let config_only = bridge_config(&home).exists() && removable(&home).is_empty();
    if home.exists() && !config_only {
        let h = home.clone();
        steps.push(step(home.clone(), String::new(), move || remove_home(&h)));
    }

    let mut done = Uninstalled { removed: Vec::new(), failed: Vec::new() };
    if steps.is_empty() {
        ctx.show(&done);
        return Ok(());
    }
    ctx.out.note("This uninstalls rant-cli and what it installed, removing:");
    steps.iter().for_each(|s| ctx.out.note(&format!("  {}", shown(s))));
    ctx.out.note("Workspaces stay as they are, and running nodes keep running.");
    if bridge_config(&home).exists() {
        ctx.out.note(&format!("{} stays, it holds your bridge passphrase and settings.", bridge_config(&home).display()));
    }
    if !ctx.confirm("Uninstall?") {
        return Err(Failure::Reported);
    }
    for s in &steps {
        match (s.remove)() {
            Ok(()) => done.removed.push(shown(s)),
            Err(e) => {
                e.messages().iter().for_each(|m| ctx.out.warn(m));
                done.failed.push(shown(s));
            }
        }
    }
    let any_failed = !done.failed.is_empty();
    ctx.show(&done);
    if any_failed { Err(Failure::Reported) } else { Ok(()) }
}
