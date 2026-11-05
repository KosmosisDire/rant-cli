//! Skills, the folders that teach AI coding agents: a package's own skill folder, and the skill a
//! library's release carries, which rant add gives. Each is linked into every agent's folder.

use std::path::{Path, PathBuf};

use rant_config::paths;
use serde_json::{Value, json};

use super::{Download, is_link, make_link, unlink, zip};
use crate::agents;
use crate::context::Context;
use crate::failure::{Result, failed};
use crate::files;
use crate::github::{self, Release};

/// The folders the agents found read skills from, each with the agents that read it.
fn readers() -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for a in agents::detected() {
        match out.iter_mut().find(|(folder, _)| *folder == a.skills) {
            Some((_, names)) => *names += &format!(", {}", a.name),
            None => out.push((a.skills, a.name.to_string())),
        }
    }
    out
}

pub fn any_link(links: &[PathBuf]) -> bool {
    links.iter().any(|l| is_link(l))
}

fn none_found(ctx: &Context, name: &str) {
    ctx.out.note(&format!("found no AI coding agent here to give the skill of {name} to"));
}

/// Whether to give the skill of name to the agents found, which the user answers. False with
/// none found, after a note saying so.
pub fn ask(ctx: &Context, name: &str) -> bool {
    let names: Vec<String> = readers().into_iter().map(|(_, n)| n).collect();
    if names.is_empty() {
        none_found(ctx, name);
        return false;
    }
    ctx.confirm(&format!("Also give the skill of {name} to {}?", names.join(", ")))
}

/// Links skill into each agent's folder under its name, each underscore a hyphen as skill names
/// need, replacing what is there. Returns the links, none and had dropped without SKILL.md.
pub fn link(ctx: &Context, name: &str, skill: &Path, had: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !skill.join("SKILL.md").is_file() {
        for l in had.iter().filter(|l| is_link(l)) {
            unlink(l)?;
        }
        return Ok(out);
    }
    let found = readers();
    if found.is_empty() {
        none_found(ctx, name);
    }
    let folder_name = name.replace('_', "-");
    for (folder, names) in found {
        let link = folder.join(&folder_name);
        let made = (|| {
            if !is_link(&link) || !had.contains(&link) {
                if is_link(&link) {
                    unlink(&link)?;
                }
                let _ = std::fs::remove_dir_all(&link);
                let _ = std::fs::create_dir_all(&folder);
                make_link(&link, skill)?;
                ctx.out.note(&format!("linked the skill of {name} for {names} as {}", paths::slash(&link)));
            }
            Ok::<(), crate::failure::Failure>(())
        })();
        match made {
            Ok(()) => out.push(link),
            Err(e) => ctx.out.warn(&format!("{names}: {}", e.messages().join(", "))),
        }
    }
    // a link in a folder no agent found reads now stays recorded, so remove still drops it
    out.extend(had.iter().filter(|l| is_link(l) && !out.contains(l)).cloned().collect::<Vec<_>>());
    Ok(out)
}

/// The skill rant add gave a library, kept in rant's home with a record beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub repo: String,
    pub version: String,
    pub links: Vec<PathBuf>,
}

fn skills_dir(home: &Path) -> PathBuf {
    home.join("skills")
}

pub fn skill(home: &Path, name: &str) -> Option<Skill> {
    let j: Value = serde_json::from_slice(&std::fs::read(skills_dir(home).join(format!("{name}.json"))).ok()?).ok()?;
    j.is_object().then_some(())?;
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let links = j["links"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
    Some(Skill { name: name.to_string(), repo: text(&j["repo"]), version: text(&j["version"]), links })
}

/// Every library skill, by name.
pub fn skills(home: &Path) -> Vec<Skill> {
    let mut out: Vec<Skill> = std::fs::read_dir(skills_dir(home))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| skill(home, &e.path().file_stem()?.to_string_lossy()))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn save(home: &Path, s: &Skill) -> Result<()> {
    let j = json!({"repo": s.repo, "version": s.version, "links": s.links.iter().map(|l| paths::slash(l)).collect::<Vec<_>>()});
    let file = skills_dir(home).join(format!("{}.json", s.name));
    std::fs::write(&file, serde_json::to_string_pretty(&j).unwrap_or_default() + "\n").map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(&file))))
}

/// Gives the skill a library's release carries to the agents found, or moves it to that
/// release. One never given asks first, and only when may_ask.
pub fn give(ctx: &Context, home: &Path, name: &str, repo: &str, r: &Release, may_ask: bool) -> Result<()> {
    let Some(asset) = r.asset(&format!("{name}-{}-skill.zip", r.version)) else { return Ok(()) };
    let had = skill(home, name);
    let links = had.as_ref().map(|h| h.links.clone()).unwrap_or_default();
    let given = any_link(&links);
    if given && had.as_ref().is_some_and(|h| h.version == r.version || github::newer(&h.version, &r.version)) {
        return Ok(());
    }
    if !(given || (may_ask && ask(ctx, name))) {
        return Ok(());
    }
    let (folder, staging) = (skills_dir(home).join(name), skills_dir(home).join(format!(".{name}.new")));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::create_dir_all(skills_dir(home));
    let unpacked = Download::new(ctx, home, Some(asset)).and_then(|zip_file| zip::extract(&zip_file.path, &staging));
    if let Err(e) = unpacked.and_then(|_| {
        if staging.join("SKILL.md").is_file() { Ok(()) } else { Err(failed(format!("{} holds no SKILL.md at its top, so it is no skill", asset.name))) }
    }) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = std::fs::remove_dir_all(&folder);
    files::rename(&staging, &folder).map_err(|e| failed(format!("cannot place {}: {e}", paths::slash(&folder))))?;
    ctx.out.note(&format!("installed the skill of {name} {}", r.version));
    let target = std::path::absolute(&folder).unwrap_or(folder);
    let links = link(ctx, name, &target, &links)?;
    save(home, &Skill { name: name.into(), repo: repo.into(), version: r.version.clone(), links })
}

/// Drops a library skill's links, its folder and its record.
pub fn remove(ctx: &Context, home: &Path, s: &Skill) -> Result<()> {
    for l in s.links.iter().filter(|l| is_link(l)) {
        unlink(l)?;
    }
    let _ = std::fs::remove_dir_all(skills_dir(home).join(&s.name));
    let _ = std::fs::remove_file(skills_dir(home).join(format!("{}.json", s.name)));
    ctx.out.note(&format!("removed the skill of {}", s.name));
    Ok(())
}
