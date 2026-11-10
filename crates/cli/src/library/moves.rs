//! Bringing packages to a library's release: a library added to a folder's build files, or every
//! use in a workspace moved, with the Python wheel, the C# package, the sketchbook copy and the
//! Engineer libraries that need more than an edit.

use std::path::{Path, PathBuf};

use rant_config::database;
use rant_process::Command;

use super::edit;
use super::{Kind, Library, Use, arduino, in_folder, plcnext, uses::dist_name};
use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::github::{self, Asset, Release};
use crate::home::{env_path, home_dir};
use crate::output::{Style, alternatives};
use crate::packages::sources::{Sources, from_repo};
use crate::packages::{Source, skills};

/// A source as a library, refused when it names a package or a folder.
fn of(s: &Source) -> Result<Library> {
    let Some(names) = &s.library else { return Err(failed(format!("{} is a package, install it with `rant install {}`", s.name, s.name))) };
    if s.repo.is_empty() {
        return Err(failed(format!("{} is a folder in config.rant, and a library comes from a repo's releases", s.name)));
    }
    Ok(Library { name: s.name.clone(), repo: s.repo.clone(), names: names.clone() })
}

/// A library by name from the sources, or by its repo's link.
pub fn find(ctx: &Context, sources: &mut Sources, word: &str) -> Result<Library> {
    let repo = if database::is_name(word) { None } else { database::repo_of(word) };
    if !database::is_name(word) && repo.is_none() {
        return Err(usage(format!("`{word}` is no library name, which is lowercase letters, digits and _, and no GitHub repo")));
    }
    if let Some(r) = repo {
        return of(&from_repo(&r)?);
    }
    match sources.find(word)? {
        Some(s) => of(&s),
        None => {
            sources.problems().iter().for_each(|p| ctx.out.warn(p));
            Err(failed(format!("no library `{word}` in config.rant or the package databases, `rant ls libraries --available` lists them")))
        }
    }
}

/// Every library the sources name.
pub fn all(sources: &mut Sources) -> Result<Vec<Library>> {
    let offers = sources.offers("")?;
    offers.iter().filter(|o| o.source.library.is_some() && !o.source.repo.is_empty()).map(|o| of(&o.source)).collect()
}

/// The wheel of a release for the platform this rant runs on, else one for any platform.
fn wheel<'r>(r: &'r Release, lib: &Library) -> Option<&'r Asset> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "win_arm64",
        ("windows", _) => "win_amd64",
        ("macos", _) => "macosx",
        (_, "aarch64") => "manylinux_2_28_aarch64",
        _ => "manylinux_2_28_x86_64",
    };
    let prefix = format!("{}-", dist_name(lib));
    let wheels: Vec<&Asset> = r.assets.iter().filter(|a| a.name.to_lowercase().starts_with(&prefix) && a.name.to_lowercase().ends_with(".whl")).collect();
    wheels.iter().find(|a| a.name.to_lowercase().contains(platform)).or_else(|| wheels.iter().find(|a| a.name.to_lowercase().ends_with("-any.whl"))).copied()
}

/// Makes the venv when it is missing, then pip installs the release's wheel into it.
fn install_python(ctx: &Context, u: &Use, lib: &Library, r: &Release) -> Result<()> {
    let Some(w) = wheel(r, lib) else { return Err(failed(format!("{} {} has no Python wheel for this platform", lib.name, r.version))) };
    if !u.venv_exists {
        ctx.out.note(&format!("creating the venv {}", ctx.shown(&u.file)));
        let was_there = u.file.exists();
        let argv = u.python.iter().cloned().chain(["-m".into(), "venv".into(), u.file.to_string_lossy().into_owned()]);
        if rant_process::run(&Command::new(argv, &u.dir)) != Ok(0) {
            // half a venv would pass for one next time
            if !was_there {
                let _ = std::fs::remove_dir_all(&u.file);
            }
            let hint = if cfg!(target_os = "linux") { ", on Debian and Ubuntu install python3-venv first" } else { "" };
            return Err(failed(format!("could not create the venv {}{hint}", ctx.shown(&u.file))));
        }
    }
    let pip = [u.venv_python.to_string_lossy().as_ref(), "-m", "pip", "install", "--upgrade", "--disable-pip-version-check", "--quiet", &w.url].map(str::to_string);
    if rant_process::run(&Command::new(pip, &u.dir)) != Ok(0) {
        return Err(failed(format!("pip could not install {} {}", lib.name, r.version)));
    }
    Ok(())
}

/// The user's NuGet config, where dotnet keeps its package sources.
fn nuget_config() -> Option<PathBuf> {
    if cfg!(windows) { env_path("APPDATA").map(|a| a.join("NuGet").join("NuGet.Config")) } else { home_dir().map(|h| h.join(".nuget").join("NuGet").join("NuGet.Config")) }
}

/// The folder rant gives dotnet as the NuGet source `rant`, with its native separators.
pub fn nuget_feed(home: &Path) -> PathBuf {
    let feed = home.join("nuget").to_string_lossy().into_owned();
    PathBuf::from(if cfg!(windows) { feed.replace('/', "\\") } else { feed })
}

pub fn nuget_feed_added(home: &Path) -> bool {
    let text = nuget_config().and_then(|c| std::fs::read_to_string(c).ok()).unwrap_or_default();
    text.contains(&*nuget_feed(home).to_string_lossy())
}

/// A library's C# package comes with its release, not from nuget.org, so it goes to a feed
/// under rant's home that dotnet is told about once.
fn install_nupkg(ctx: &Context, home: &Path, lib: &Library, r: &Release) -> Result<()> {
    let name = format!("{}.{}.nupkg", lib.names.csharp.as_deref().unwrap_or_default(), r.version);
    let Some(a) = r.asset(&name) else { return Err(failed(format!("{} {} has no {name}", lib.name, r.version))) };
    let feed = nuget_feed(home);
    if !feed.join(&name).exists() {
        github::download(&a.url, &feed.join(&name), false)?;
    }
    if nuget_feed_added(home) {
        return Ok(());
    }
    let cwd = ctx.cwd()?;
    if rant_process::find_program("dotnet", cwd).is_none() {
        return Err(failed("dotnet not found, install the .NET SDK"));
    }
    let shown = feed.to_string_lossy().into_owned();
    ctx.out.note(&format!("adding the NuGet feed {shown}"));
    let add = Command::new(["dotnet", "nuget", "add", "source", &shown, "--name", "rant"].map(str::to_string), cwd);
    if rant_process::run(&add) != Ok(0) {
        return Err(failed(format!("dotnet could not add the feed {shown}")));
    }
    Ok(())
}

/// The release file a kind of use takes the library from, None for one that needs no file.
fn needs(kind: Kind, library: &str, version: &str) -> Option<String> {
    match kind {
        Kind::PlatformIO | Kind::Arduino => Some(arduino::zip_name(library, version)),
        Kind::Plcx | Kind::Engineer => Some(plcnext::file_name(library, version)),
        _ => None,
    }
}

/// The file a use needs that the release lacks.
fn lacks(u: &Use, r: &Release) -> Option<String> {
    needs(u.kind, &u.library, &r.version).filter(|f| r.asset(f).is_none())
}

/// A use the release would change. An unpinned pyproject dependency takes what its venv has,
/// and an unpinned Cargo one what its source has, and Arduino and Engineer need the release's
/// file. Only a version asked for moves one back.
fn changes(u: &Use, r: &Release, asked: bool) -> bool {
    if lacks(u, r).is_some() || !u.wanted || u.version.as_deref() == Some(r.version.as_str()) || (matches!(u.how, "pyproject" | "Cargo") && u.version.is_none()) {
        return false;
    }
    asked || u.version.as_ref().is_none_or(|v| !github::newer(v, &r.version))
}

/// Brings one use to the release. Returns what it did, empty when there was nothing to do.
fn bring(ctx: &Context, home: &Path, u: &Use, lib: &Library, r: &Release, asked: bool) -> Result<String> {
    if !changes(u, r, asked) {
        // the feed may still lack it
        if u.how == "PackageReference" {
            install_nupkg(ctx, home, lib, r)?;
        }
        return Ok(String::new());
    }
    let moved = format!("{} -> {} in {}", u.version.as_deref().unwrap_or("none"), r.version, ctx.shown(&u.file));
    if u.how == "venv" {
        install_python(ctx, u, lib, r)?;
        return Ok(moved);
    }
    if u.kind == Kind::Arduino {
        return Ok(if arduino::install(ctx, home, lib, r)?.is_some() { moved } else { String::new() });
    }
    if u.kind == Kind::Engineer {
        plcnext::install(ctx, home, lib, r)?;
        return Ok(moved);
    }
    if u.kind == Kind::Plcx {
        plcnext::into_workspace(ctx, home, &u.dir, lib, r)?;
        return Ok(moved);
    }
    edit::set_version(&u.file, lib, &r.version)?;
    if u.kind == Kind::CSharp {
        install_nupkg(ctx, home, lib, r)?;
    }
    Ok(moved)
}

/// The build files in a folder that could take the library but do not name it yet. A sketch takes
/// the sketchbook and an Engineer archive the user libraries. Each of the last four needs the
/// release's file.
fn missing(dir: &Path, lib: &Library, uses: &[Use], r: &Release) -> Vec<(Kind, PathBuf)> {
    let used = |f: &Path| uses.iter().any(|u| u.file == f);
    let mut out = Vec::new();
    let cmake = dir.join("CMakeLists.txt");
    if lib.supports(Kind::CMake) && cmake.is_file() && !used(&cmake) {
        out.push((Kind::CMake, cmake));
    }
    if lib.supports(Kind::CSharp) {
        let mut projects: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "csproj") && !used(p)).collect();
        projects.sort();
        out.extend(projects.into_iter().map(|p| (Kind::CSharp, p)));
    }
    let cargo = dir.join("Cargo.toml");
    if lib.supports(Kind::Rust) && cargo.is_file() && !used(&cargo) {
        out.push((Kind::Rust, cargo));
    }
    let ini = dir.join("platformio.ini");
    let zip = arduino::carried(r, lib).is_some();
    if zip && ini.is_file() && !used(&ini) {
        out.push((Kind::PlatformIO, ini));
    }
    if zip && arduino::is_sketch(dir) && !uses.iter().any(|u| u.kind == Kind::Arduino) {
        // with no sketchbook the path stays empty and install says so
        out.push((Kind::Arduino, arduino::sketchbook().map(|b| b.join("libraries")).unwrap_or_default()));
    }
    let (pcwlx, pcwex) = (lib.supports(Kind::Plcx) && plcnext::carried(r, lib).is_some(), dir.join(plcnext::WORKSPACE));
    if pcwlx && pcwex.is_file() && !used(&pcwex) {
        out.push((Kind::Plcx, pcwex));
    }
    if pcwlx && plcnext::is_project(dir) && !uses.iter().any(|u| u.kind == Kind::Engineer) {
        out.push((Kind::Engineer, plcnext::folders().first().cloned().unwrap_or_default()));
    }
    out
}

/// The build files a library goes in, as a sentence names them: "CMakeLists.txt or Cargo.toml".
fn takes(lib: &Library) -> String {
    let files = [(Kind::CMake, "CMakeLists.txt"), (Kind::CSharp, "C# project"), (Kind::Rust, "Cargo.toml"), (Kind::PlatformIO, "platformio.ini"), (Kind::Arduino, "Arduino sketch"), (Kind::Plcx, "plcx workspace"), (Kind::Engineer, "PLCnext Engineer project"), (Kind::Python, "Python file")];
    alternatives(&files.iter().filter(|(k, _)| lib.supports(*k)).map(|(_, f)| *f).collect::<Vec<_>>())
}

fn report(ctx: &Context, d: &Path, k: Kind, what: &str) {
    ctx.out.line(&format!("  {} {}: {what}", ctx.shown(d), k.name()));
}

#[derive(Default)]
struct Fetches {
    cmake: bool,
    cargo: bool,
    pio: bool,
    plcx: bool,
    engineer: bool,
}

impl Fetches {
    fn after(&mut self, k: Kind) {
        self.cmake |= k == Kind::CMake;
        self.cargo |= k == Kind::Rust;
        self.pio |= k == Kind::PlatformIO;
        self.plcx |= k == Kind::Plcx;
        self.engineer |= k == Kind::Engineer;
    }

    fn notes(&self, ctx: &Context) {
        if self.cmake {
            ctx.out.note("CMake fetches it on the next configure, which `rant build` runs");
        }
        if self.cargo {
            ctx.out.note("Cargo fetches it on the next build, which `rant build` runs");
        }
        if self.pio {
            ctx.out.note("PlatformIO fetches it on the next build");
        }
        if self.plcx {
            ctx.out.note("`plcx build` packs it into the project");
        }
        if self.engineer {
            ctx.out.note("add it to the project in PLCnext Engineer from the user libraries");
        }
    }
}

/// Adds a library to the build files in dir that lack it and moves the ones naming it to the
/// release, then offers its skill. asked may move one back. False when one failed.
pub fn add(ctx: &Context, home: &Path, dir: &Path, lib: &Library, r: &Release, asked: bool) -> Result<bool> {
    let uses = in_folder(dir, std::slice::from_ref(lib), true);
    let adds = missing(dir, lib, &uses, r);
    if uses.is_empty() && adds.is_empty() {
        if arduino::carried(r, lib).is_none() && (dir.join("platformio.ini").is_file() || arduino::is_sketch(dir)) {
            return Err(failed(format!("{} {} carries no {}, so it has no Arduino library", lib.name, r.version, arduino::zip_name(&lib.name, &r.version))));
        }
        if lib.supports(Kind::Plcx) && plcnext::carried(r, lib).is_none() && (plcnext::is_workspace(dir) || plcnext::is_project(dir)) {
            return Err(failed(format!("{} {} carries no {}, so it has no PLCnext Engineer library", lib.name, r.version, plcnext::file_name(&lib.name, &r.version))));
        }
        return Err(failed(format!("no {} in {} to add {} to", takes(lib), ctx.shown(dir), lib.name)));
    }
    let moves: Vec<&Use> = uses.iter().filter(|u| changes(u, r, asked)).collect();
    if adds.is_empty() && moves.is_empty() {
        let lacking = uses.iter().filter(|u| u.version.as_deref() != Some(r.version.as_str())).find_map(|u| lacks(u, r));
        if let Some(file) = lacking {
            ctx.out.note(&format!("{} {} carries no {file}, so {} stays as it is", lib.name, r.version, ctx.shown(dir)));
            return Ok(true);
        }
        match uses.iter().filter_map(|u| u.version.as_ref()).find(|v| **v != r.version) {
            Some(newer) => ctx.out.note(&format!("{} uses {} {newer}, newer than the release {}, so it stays", ctx.shown(dir), lib.name, r.version)),
            None => ctx.out.note(&format!("{} uses {} {} already", ctx.shown(dir), lib.name, r.version)),
        }
        skills::give(ctx, home, &lib.name, &lib.repo, r, true)?;
        return Ok(true);
    }
    ctx.out.line(&ctx.out.paint_out(Style::Bold, &format!("{} {}", lib.name, r.version)));
    let (mut ok, mut fetches) = (true, Fetches::default());
    for u in moves {
        match bring(ctx, home, u, lib, r, asked) {
            Ok(what) => {
                report(ctx, &u.dir, u.kind, &what);
                fetches.after(u.kind);
            }
            Err(e) => {
                ctx.out.warn(&format!("{} {}: {}", ctx.shown(&u.dir), u.kind.name(), e.messages().join(", ")));
                ok = false;
            }
        }
    }
    for (kind, file) in adds {
        let added = match kind {
            Kind::Arduino => arduino::install(ctx, home, lib, r).map(|placed| placed.map(|_| String::new())),
            Kind::Engineer => plcnext::install(ctx, home, lib, r).map(|_| Some(String::new())),
            Kind::Plcx => plcnext::into_workspace(ctx, home, dir, lib, r).map(|_| Some(String::new())),
            Kind::CSharp => edit::add(kind, &file, lib, &r.version, &r.tag).and_then(|hint| install_nupkg(ctx, home, lib, r).map(|_| Some(hint))),
            _ => edit::add(kind, &file, lib, &r.version, &r.tag).map(Some),
        };
        match added {
            Ok(None) => {}
            Ok(Some(hint)) => {
                report(ctx, dir, kind, &format!("added to {}", ctx.shown(&file)));
                if !hint.is_empty() {
                    ctx.out.note(&format!("    link it to your target: {hint}"));
                }
                fetches.after(kind);
            }
            Err(e) => {
                ctx.out.warn(&format!("{} {}: {}", ctx.shown(dir), kind.name(), e.messages().join(", ")));
                ok = false;
            }
        }
    }
    fetches.notes(ctx);
    skills::give(ctx, home, &lib.name, &lib.repo, r, true)?;
    Ok(ok)
}

/// Moves every use of the libraries to each one's latest release, after listing them and one
/// question, never to an older release. False when one failed, after a warning.
pub fn update(ctx: &Context, home: &Path, uses: &[Use], libs: &[Library]) -> Result<bool> {
    let used: Vec<&Library> = libs.iter().filter(|l| uses.iter().any(|u| u.library == l.name)).collect();
    let latest = github::latest(&used.iter().map(|l| l.repo.clone()).collect::<Vec<_>>())?;

    // every package under a folder is a lot to change unasked, so the list comes first
    let mut ok = true;
    let mut n = 0;
    for (lib, r) in used.iter().zip(&latest) {
        let Some(r) = r else {
            ctx.out.warn(&format!("{} has no release in {}", lib.name, lib.repo));
            ok = false;
            continue;
        };
        ctx.out.line(&ctx.out.paint_out(Style::Bold, &format!("{} {}", lib.name, r.version)));
        let before = n;
        for u in uses.iter().filter(|u| u.library == lib.name && changes(u, r, false)) {
            report(ctx, &u.dir, u.kind, &format!("{} -> {} in {}", u.version.as_deref().unwrap_or("none"), r.version, ctx.shown(&u.file)));
            n += 1;
        }
        if n == before {
            ctx.out.line(&format!("  {}", ctx.out.paint_out(Style::Dim, &format!("everything here is at {} or newer", r.version))));
        }
    }
    if n > 0 && !ctx.confirm(&format!("Update {n} {}?", if n == 1 { "package" } else { "packages" })) {
        ctx.out.note("nothing was changed");
        return Ok(false);
    }

    let (mut fetches, mut done) = (Fetches::default(), 0);
    for (lib, r) in used.iter().zip(&latest) {
        let Some(r) = r else { continue };
        for u in uses.iter().filter(|u| u.library == lib.name) {
            match bring(ctx, home, u, lib, r, false) {
                Ok(what) if what.is_empty() => {}
                Ok(_) => {
                    done += 1;
                    fetches.after(u.kind);
                }
                Err(e) => {
                    ctx.out.warn(&format!("{} {}: {}", ctx.shown(&u.dir), u.kind.name(), e.messages().join(", ")));
                    ok = false;
                }
            }
        }
    }
    if done > 0 {
        ctx.out.line(&format!("updated {done} {}", if done == 1 { "package" } else { "packages" }));
    }
    fetches.notes(ctx);
    for (lib, r) in used.iter().zip(&latest) {
        let Some(r) = r else { continue };
        if let Err(e) = skills::give(ctx, home, &lib.name, &lib.repo, r, false) {
            ctx.out.warn(&e.messages().join(", "));
            ok = false;
        }
    }
    Ok(ok)
}
