//! Installing and removing packages in rant's home: a release's files, a local build folder
//! linked, or an archive unpacked, each with a record beside it.

use std::path::{Path, PathBuf};

use rant_config::package::PACKAGE_FILE;
use rant_config::paths;
use rant_process::Command;

use super::sources::{Sources, carried, is_archive, named, read_text};
use super::vsix::{self, Target, Wanted};
use super::pcwlx;
use super::{Download, Record, Source, dir, is_link, make_link, record, record_file, save, skills, unlink, zip};
use crate::context::Context;
use crate::failure::{Failure, Result, failed};
use crate::files;
use crate::github::{self, platform, raw_url};
use crate::nodes;
use crate::output::spoken;
use crate::state;

fn shown(s: &Source) -> String {
    if s.repo.is_empty() { paths::slash(&s.path) } else { s.repo.clone() }
}

/// Removes what earlier replacements of this package could not, now that it may be free.
fn sweep(home: &Path, name: &str) {
    let prefix = format!(".{name}.old-");
    for e in std::fs::read_dir(dir(home)).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(&prefix) && !is_link(&e.path()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Moves the package's folder out of the way under a dot name the config skips, to go once
/// nothing holds it, or drops its link.
fn retire(home: &Path, name: &str) -> Result<()> {
    let target = dir(home).join(name);
    if is_link(&target) {
        return unlink(&target);
    }
    if !target.exists() {
        return Ok(());
    }
    let old = dir(home).join(format!(".{name}.old-{}", files::stamp()));
    files::rename(&target, &old).map_err(|e| failed(format!("cannot replace {}: {e}", paths::slash(&target))))?;
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// The nodes any workspace started whose program or folder lies in folder.
fn running_from(home: &Path, folder: &Path) -> Vec<String> {
    let states = state::remembered(home).into_iter().filter_map(|d| nodes::snapshot(home, &d).ok());
    states.flat_map(|st| st.instances.into_iter().filter(|i| i.runs_from(folder)).map(|i| i.name)).collect()
}

/// A package's folder cannot change under its running nodes: Windows refuses to move a folder a
/// process works in, and elsewhere they would run what is gone. A link may change.
pub fn refuse_running(home: &Path, name: &str) -> Result<()> {
    let folder = dir(home).join(name);
    if is_link(&folder) {
        return Ok(());
    }
    let running = running_from(home, &folder);
    if running.is_empty() {
        return Ok(());
    }
    let (runs, them) = if running.len() == 1 { ("runs", "it") } else { ("run", "them") };
    Err(failed(format!("{} {runs} from {name}, stop {them} first with `rant stop`", spoken(&running))))
}

/// The system's tar, which opens .tar.gz on Windows 10 and later too. On Windows its own, since
/// an MSYS tar earlier on PATH reads C: as a remote host.
fn tar_program() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let system = rant_process::system_dir().join("tar.exe");
        if system.is_file() {
            return Ok(system);
        }
    }
    rant_process::find_program("tar", &std::env::temp_dir()).ok_or_else(|| failed("unpacking needs tar, which is not on PATH"))
}

fn refuse_other_name(found: &str, name: &str, place: &str) -> Result<()> {
    if found != name {
        return Err(failed(format!("{place} holds the package {found}, not {name}")));
    }
    Ok(())
}

/// Fills a new folder for the package, an archive unpacked or only the package.rant text of a
/// release without one, with a skill zip unpacked as its skill folder, then puts it in place.
fn place(home: &Path, name: &str, archive: Option<&Path>, text: &str, skill: Option<&Path>) -> Result<()> {
    let staging = dir(home).join(format!(".{name}.new"));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::create_dir_all(&staging);
    let filled = (|| {
        let from = archive.map(paths::slash).unwrap_or_default();
        match archive {
            None => std::fs::write(staging.join(PACKAGE_FILE), text).map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(&staging))))?,
            Some(a) => {
                let tar = tar_program()?.to_string_lossy().into_owned();
                let unpack = Command::new([tar, "-xzf".into(), a.to_string_lossy().into_owned(), "-C".into(), staging.to_string_lossy().into_owned()], std::env::temp_dir());
                if rant_process::run(&unpack) != Ok(0) {
                    return Err(failed(format!("cannot unpack {from}")));
                }
            }
        }
        if !staging.join(PACKAGE_FILE).is_file() {
            return Err(failed(format!("{from} holds no package.rant at its top, so it is no package archive")));
        }
        if let Some(sk) = skill {
            let _ = std::fs::remove_dir_all(staging.join("skill"));
            zip::extract(sk, &staging.join("skill"))?;
            if !staging.join("skill").join("SKILL.md").is_file() {
                return Err(failed(format!("{} holds no SKILL.md at its top, so it is no skill", paths::slash(sk))));
            }
        }
        if archive.is_some() {
            let (found, _) = named(&dir(home).join(name), &from, read_text(&staging.join(PACKAGE_FILE)))?;
            refuse_other_name(&found, name, &from)?;
        }
        Ok(())
    })();
    if let Err(e) = filled {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    retire(home, name)?;
    let target = dir(home).join(name);
    files::rename(&staging, &target).map_err(|e| failed(format!("cannot place {}: {e}", paths::slash(&target))))
}

/// Where an extension goes: the editor and profile wanted, else those it went into before while
/// that editor is there, else the ones vsix::target finds, which may ask.
fn extension_target(ctx: &Context, home: &Path, name: &str, wanted: &Wanted) -> Result<Target> {
    if !wanted.any()
        && let Some(h) = record(home, name)
        && !h.extension.is_empty()
        && h.editor.is_file()
    {
        return Ok(Target { editor: h.editor, profile: h.profile });
    }
    vsix::target(ctx, wanted)
}

/// Installs an extension into the target and records where. From a path the extension's own
/// version stands for the package's.
fn install_extension(ctx: &Context, home: &Path, t: &Target, file: &Path, rec: &mut Record) -> Result<()> {
    let had = record(home, &rec.name);
    let v = vsix::install(t, file)?;
    (rec.extension, rec.editor, rec.profile) = (v.id.clone(), t.editor.clone(), t.profile.clone());
    if rec.repo.is_empty() {
        rec.version = v.version.clone();
    }
    ctx.out.note(&format!("installed the extension {} {} through {}, reload the editor window to start it", v.id, v.version, vsix::shown(t)));
    if let Some(h) = had
        && !h.extension.is_empty()
        && (h.editor != t.editor || h.profile != t.profile)
    {
        ctx.out.note(&format!("it stays in {} as well, where rant no longer updates it", vsix::shown(&Target { editor: h.editor, profile: h.profile })));
    }
    Ok(())
}

/// Places a PLCnext Engineer library where Engineer takes user libraries from, in place of the
/// copies placed before.
fn install_library(ctx: &Context, home: &Path, file: &Path, rec: &mut Record) -> Result<()> {
    let folders = pcwlx::folders();
    if folders.is_empty() {
        ctx.out.note(&format!("{} carries a PLCnext Engineer library, which goes only where Engineer runs, on Windows", rec.name));
        return Ok(());
    }
    let had = record(home, &rec.name).map(|h| h.libraries).unwrap_or_default();
    rec.libraries = pcwlx::install(file, &folders)?;
    let gone: Vec<PathBuf> = had.into_iter().filter(|h| !rec.libraries.iter().any(|p| paths::same(p, h))).collect();
    pcwlx::remove(&gone)?;
    for p in &rec.libraries {
        let name = p.file_stem().unwrap_or_default().to_string_lossy();
        ctx.out.note(&format!("installed the PLCnext Engineer library {name} as {}, add it to a project from Engineer's user libraries", paths::slash(p)));
    }
    Ok(())
}

fn no_extension(name: &str) -> Failure {
    failed(format!("{name} is no VS Code extension, so --editor and --profile do not apply"))
}

/// A release of a repo, the files it carries found by their names.
fn install_release(ctx: &Context, home: &Path, s: &Source, version: &str, wanted: &Wanted, rec: &mut Record) -> Result<()> {
    let r = github::release(&s.repo, version)?;
    if let Some(h) = record(home, &s.name)
        && h.repo.eq_ignore_ascii_case(&s.repo)
        && h.named_by != "--from"
        && h.version == r.version
        && dir(home).join(&s.name).is_dir()
        && (!wanted.any() || h.extension.is_empty())
    {
        ctx.out.note(&format!("{} {} is already installed", s.name, r.version));
        *rec = Record { named_by: s.named_by.clone(), ..h };
        return Ok(());
    }
    refuse_running(home, &s.name)?;
    let place_name = format!("{} at {}", s.repo, r.tag);
    let text = github::fetch(&raw_url(&s.repo, &r.tag, PACKAGE_FILE))?;
    let (found, _) = named(Path::new(&s.repo), &place_name, text.clone())?;
    refuse_other_name(&found, &s.name, &place_name)?;
    let c = carried(&r, &s.name, platform());
    let (release, prefix) = (format!("the {} {} release", s.name, r.version), format!("{}-{}", s.name, r.version));
    if c.archive.is_none() && !c.platforms.is_empty() {
        return Err(failed(format!("{release} is built for {}, not for {}", c.platforms.join(", "), platform())));
    }
    if c.archive.is_none() && c.extension.is_none() && c.library.is_none() && c.skill.is_none() {
        return Err(failed(format!("{release} carries no {prefix}-{}.tar.gz, {prefix}.vsix, {prefix}.pcwlx or {prefix}-skill.zip, so it has nothing to install", platform())));
    }
    if c.extension.is_none() && wanted.any() {
        return Err(no_extension(&s.name));
    }
    // the editor and profile first, since picking one may ask
    let target = c.extension.as_ref().map(|_| extension_target(ctx, home, &s.name, wanted)).transpose()?;
    let archive = Download::new(ctx, home, c.archive.as_ref())?;
    let extension = Download::new(ctx, home, c.extension.as_ref())?;
    let library = Download::new(ctx, home, c.library.as_ref())?;
    let skill = Download::new(ctx, home, c.skill.as_ref())?;
    if let (Some(t), Some(file)) = (&target, extension.file()) {
        install_extension(ctx, home, t, file, rec)?;
    }
    if let Some(file) = library.file() {
        install_library(ctx, home, file, rec)?;
    }
    place(home, &s.name, archive.file(), &text.unwrap_or_default(), skill.file())?;
    rec.version = r.version.clone();
    ctx.out.note(&format!("installed {} {} from {}", s.name, r.version, s.repo));
    Ok(())
}

/// The newest extension of a package a build folder holds, which its build wrote last, named as
/// a release names it.
fn newest_vsix(folder: &Path, name: &str) -> Option<PathBuf> {
    let candidates = std::fs::read_dir(folder).into_iter().flatten().flatten().filter(|e| {
        let file = e.file_name().to_string_lossy().into_owned();
        file.starts_with(&format!("{name}-")) && file.ends_with(".vsix")
    });
    candidates.max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok()).map(|e| e.path())
}

/// A local build folder, linked so each new build counts at once, or an archive, unpacked.
fn install_path(ctx: &Context, home: &Path, s: &Source, wanted: &Wanted, rec: &mut Record) -> Result<()> {
    let from = paths::slash(&s.path);
    if s.path.is_dir() {
        let (found, _) = named(&s.path, &from, read_text(&s.path.join(PACKAGE_FILE)))?;
        refuse_other_name(&found, &s.name, &from)?;
        let extension = newest_vsix(&s.path, &s.name);
        match &extension {
            Some(file) => install_extension(ctx, home, &extension_target(ctx, home, &s.name, wanted)?, file, rec)?,
            None if wanted.any() => return Err(no_extension(&s.name)),
            None => {}
        }
        let library = pcwlx::newest(&s.path);
        if let Some(file) = &library {
            install_library(ctx, home, file, rec)?;
        }
        retire(home, &s.name)?;
        let target = paths::normalize(&std::path::absolute(&s.path).unwrap_or_else(|_| s.path.clone()));
        make_link(&dir(home).join(&s.name), &target)?;
        let tail = if extension.is_none() && library.is_none() { ", so its builds there count at once" } else { "" };
        ctx.out.note(&format!("linked {} to {from}{tail}", s.name));
        return Ok(());
    }
    if wanted.any() {
        return Err(failed("--editor and --profile go with a VS Code extension, which an archive holds none of"));
    }
    if is_archive(&s.path) && s.path.is_file() {
        place(home, &s.name, Some(&s.path), "", None)?;
        ctx.out.note(&format!("installed {} from {from}", s.name));
        return Ok(());
    }
    if !s.path.exists() {
        return Err(failed(format!("no folder or archive {from} for {}", s.name)));
    }
    Err(failed(format!("{from} is no package folder or .tar.gz archive")))
}

/// Whether a package is more than a skill: an extension or other files beside its skill.
fn more_than_skill(home: &Path, rec: &Record) -> bool {
    !rec.extension.is_empty()
        || !rec.libraries.is_empty()
        || std::fs::read_dir(dir(home).join(&rec.name)).into_iter().flatten().flatten().any(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            file != PACKAGE_FILE && file != "skill" && !file.starts_with('.')
        })
}

/// Gives a package's skill folder to the agents found. A package that is more than a skill asks
/// first, until its skill is given.
fn link_skills(ctx: &Context, home: &Path, rec: &mut Record, had: &[PathBuf]) -> Result<()> {
    let skill = std::path::absolute(dir(home).join(&rec.name).join("skill")).unwrap_or_default();
    if skill.join("SKILL.md").is_file() && !skills::any_link(had) && more_than_skill(home, rec) && !skills::ask(ctx, &rec.name) {
        rec.skills.clear();
        return Ok(());
    }
    rec.skills = skills::link(ctx, &rec.name, &skill, had)?;
    Ok(())
}

/// Installs a package from its source, version picking a release of a repo's.
pub fn install(ctx: &Context, home: &Path, s: &Source, version: &str, wanted: &Wanted) -> Result<()> {
    if s.library.is_some() {
        return Err(failed(format!("{} is a library, add it to a package with `rant add {}`", s.name, s.name)));
    }
    let _ = std::fs::create_dir_all(dir(home));
    // a link names a package by its own word, so it never quietly takes the place of another
    let had = record(home, &s.name);
    if let Some(h) = &had {
        let same = if s.repo.is_empty() { h.repo.is_empty() && paths::same(&h.path, &s.path) } else { h.repo.eq_ignore_ascii_case(&s.repo) };
        if !same && (h.named_by == "link" || s.named_by == "link") {
            let from = if h.repo.is_empty() { paths::slash(&h.path) } else { h.repo.clone() };
            return Err(failed(format!("{} is installed from {from}, `rant remove {}` first to install it from {}", s.name, s.name, shown(s))));
        }
    }
    sweep(home, &s.name);
    let mut rec = Record { name: s.name.clone(), repo: s.repo.clone(), path: s.path.clone(), named_by: s.named_by.clone(), ..Record::default() };
    if s.repo.is_empty() {
        refuse_running(home, &s.name)?;
        install_path(ctx, home, s, wanted, &mut rec)?;
    } else {
        install_release(ctx, home, s, version, wanted, &mut rec)?;
    }
    link_skills(ctx, home, &mut rec, &had.map(|h| h.skills).unwrap_or_default())?;
    save(home, &rec)
}

/// Removes an installed package, its extension and its skill links, or a library's skill. A
/// linked folder stays as it is.
pub fn remove(ctx: &Context, home: &Path, name: &str) -> Result<()> {
    let target = dir(home).join(name);
    if !is_link(&target) && !target.exists() && !record_file(home, name).exists() {
        if let Some(s) = skills::skill(home, name) {
            return skills::remove(ctx, home, &s);
        }
        return Err(failed(format!("{name} is not installed, see `rant ls packages`")));
    }
    refuse_running(home, name)?;
    let rec = record(home, name);
    if let Some(r) = rec.as_ref().filter(|r| !r.extension.is_empty()) {
        let t = Target { editor: r.editor.clone(), profile: r.profile.clone() };
        if !t.editor.is_file() {
            ctx.out.warn(&format!("{} is gone, so the extension {} stays wherever it went", paths::slash(&t.editor), r.extension));
        } else if !vsix::installed_version(&t, &r.extension).is_empty() {
            vsix::remove(&t, &r.extension)?;
        }
    }
    if let Some(r) = &rec {
        pcwlx::remove(&r.libraries)?;
    }
    for l in rec.iter().flat_map(|r| &r.skills).filter(|l| is_link(l)) {
        unlink(l)?;
    }
    sweep(home, name);
    retire(home, name)?;
    let _ = std::fs::remove_file(record_file(home, name));
    ctx.out.note(&format!("removed {name}"));
    Ok(())
}

/// What updating an installed package would do: its source, the version it has, the version
/// it would get, and whether anything changes.
#[derive(Debug, Clone)]
pub struct Update {
    pub source: Source,
    pub had: String,
    pub to: String,
    pub needed: bool,
}

/// Where an installed package would update from. None for one rant leaves alone, after a note.
pub fn check(ctx: &Context, home: &Path, sources: &mut Sources, name: &str) -> Result<Option<Update>> {
    let target = dir(home).join(name);
    let Some(rec) = record(home, name) else {
        if !is_link(&target) && !target.exists() {
            return Err(failed(format!("{name} is not installed, `rant install {name}` installs it")));
        }
        ctx.out.note(&format!("{name} was put in {} by hand, so update leaves it alone", paths::slash(&dir(home))));
        return Ok(None);
    };
    if rec.named_by == "--from" {
        ctx.out.note(&format!("{name} came from {} through --from, so update leaves it alone", paths::slash(&rec.path)));
        return Ok(None);
    }
    let s = if rec.named_by == "link" {
        Some(Source { name: name.into(), repo: rec.repo.clone(), path: PathBuf::new(), named_by: "link".into(), library: None })
    } else {
        sources.find(name)?
    };
    let Some(s) = s else { return Err(failed(format!("no source names {name} any more, see `rant ls sources`"))) };
    let mut u = Update { source: s.clone(), had: rec.version.clone(), to: String::new(), needed: false };
    if !s.repo.is_empty() {
        u.to = github::release(&s.repo, "")?.version;
        u.needed = !rec.repo.eq_ignore_ascii_case(&s.repo) || github::newer(&u.to, &rec.version);
    } else if !paths::same(&rec.path, &s.path) {
        u.needed = true;
    } else if s.path.is_dir() {
        // a linked folder is current by itself, but its extension and library are built into files
        u.needed = !is_link(&target);
        if !rec.libraries.is_empty() {
            let Some(file) = pcwlx::newest(&s.path) else {
                return Err(failed(format!("{} holds no .pcwlx to install, build one there first", paths::slash(&s.path))));
            };
            u.needed = u.needed || pcwlx::newer(&file, &rec.libraries);
        }
        if !rec.extension.is_empty() {
            let Some(file) = newest_vsix(&s.path, name) else {
                return Err(failed(format!("{} holds no {name}-*.vsix to install, build one there first", paths::slash(&s.path))));
            };
            u.to = vsix::read(&file)?.version;
            u.needed = u.needed || u.to != rec.version;
        }
    } else {
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        u.needed = modified(&s.path) > modified(&record_file(home, name));
    }
    Ok(Some(u))
}
