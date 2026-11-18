//! The places that keep a library once per machine for every project that takes it, the Arduino
//! sketchbook and PLCnext Engineer's user libraries. rant install fills them and rant remove empties them.

use std::path::{Path, PathBuf};

use rant_config::paths;

use super::moves::{Fetches, changes};
use super::{Kind, Library, MACHINE, Use, arduino, plcnext};
use crate::context::Context;
use crate::failure::Result;
use crate::github::Release;
use crate::packages::skills;

/// Where a kind keeps the library and for whom, as a sentence names them.
fn place(k: Kind) -> (&'static str, &'static str) {
    match k {
        Kind::Arduino => ("the Arduino sketchbook", "every sketch"),
        _ => ("PLCnext Engineer's user libraries", "every project"),
    }
}

/// The kinds whose file the release carries.
pub fn carried(lib: &Library, r: &Release) -> Vec<Kind> {
    let has = |k: Kind| match k {
        Kind::Arduino => arduino::carried(r, lib).is_some(),
        _ => plcnext::carried(r, lib).is_some(),
    };
    MACHINE.into_iter().filter(|&k| lib.supports(k) && has(k)).collect()
}

/// Where rant install would put the library, None when nowhere.
pub fn offered(lib: &Library, r: &Release) -> Option<String> {
    let kinds = carried(lib, r);
    (!kinds.is_empty()).then(|| kinds.into_iter().map(|k| format!("in {} for {}", place(k).0, place(k).1)).collect::<Vec<_>>().join(" and "))
}

/// The library's copy a kind keeps.
fn copy(lib: &Library, k: Kind) -> Option<Use> {
    match k {
        Kind::Arduino => arduino::Book::load().copy(lib),
        _ => plcnext::user_copy(lib, &plcnext::folders()),
    }
}

/// Every copy of the libraries kept once per machine.
pub fn copies(libs: &[Library]) -> Vec<Use> {
    libs.iter().flat_map(|lib| MACHINE.into_iter().filter(|&k| lib.supports(k)).filter_map(|k| copy(lib, k))).collect()
}

/// Puts the release's library where each kind keeps it, noting each copy. Returns whether it placed
/// one, since a sketchbook copy that is a link stays.
pub(super) fn place_in(ctx: &Context, home: &Path, lib: &Library, r: &Release, kinds: &[Kind]) -> Result<bool> {
    let mut any = false;
    for &k in kinds {
        let placed: Vec<PathBuf> = match k {
            Kind::Arduino => arduino::install(ctx, home, lib, r)?.into_iter().collect(),
            _ => plcnext::install(ctx, home, lib, r)?,
        };
        for at in &placed {
            ctx.out.note(&format!("installed {} {} in {}", lib.name, r.version, paths::slash(at)));
        }
        any |= !placed.is_empty();
    }
    Ok(any)
}

/// Installs the library where each kind keeps it, unless the copy there is at the release or newer,
/// then offers its skill. asked may move one back.
pub fn install(ctx: &Context, home: &Path, lib: &Library, r: &Release, kinds: &[Kind], asked: bool) -> Result<()> {
    let mut moving = Vec::new();
    for &k in kinds {
        match copy(lib, k) {
            Some(u) if !changes(&u, r, asked) => match u.version.filter(|v| *v != r.version) {
                Some(newer) => ctx.out.note(&format!("{} {newer} in {} is newer than the release {}, so it stays", lib.name, place(k).0, r.version)),
                None => ctx.out.note(&format!("{} {} is in {} already", lib.name, r.version, place(k).0)),
            },
            _ => moving.push(k),
        }
    }
    if place_in(ctx, home, lib, r, &moving)? {
        let mut fetches = Fetches::default();
        moving.iter().for_each(|&k| fetches.after(k));
        fetches.notes(ctx);
    }
    skills::give(ctx, home, &lib.name, &lib.repo, r, true)
}

/// Removes every copy of the library kept once per machine, and the skill rant gave it. False when
/// there was neither.
pub fn uninstall(ctx: &Context, home: &Path, lib: &Library) -> Result<bool> {
    let mut removed: Vec<PathBuf> = Vec::new();
    for k in MACHINE.into_iter().filter(|&k| lib.supports(k)) {
        match k {
            Kind::Arduino => removed.extend(arduino::uninstall(ctx, lib)?),
            _ => removed.extend(plcnext::uninstall(lib)?),
        }
    }
    for at in &removed {
        ctx.out.note(&format!("removed {} from {}", lib.name, paths::slash(at)));
    }
    let skill = skills::skill(home, &lib.name);
    if let Some(s) = &skill {
        skills::remove(ctx, home, s)?;
    }
    Ok(!removed.is_empty() || skill.is_some())
}
