//! The packages rant install puts in the home's packages folder, each from a repo's releases or
//! a path that sources.rant or a package database names, with a record beside it.

mod install;
mod links;
pub mod skills;
pub mod sources;
pub mod vsix;
pub mod zip;

pub use install::{Update, check, install, remove};
pub use links::{is_link, make_link, unlink};

use std::path::{Path, PathBuf};

use rant_config::model::{Model, NodeType};
use rant_config::{Settings, database, package, paths};
use serde_json::{Map, Value, json};

use crate::context::Context;
use crate::failure::{Result, failed};
use crate::files;
use crate::github::{self, Asset};

/// The official package database, read after sources.rant.
pub const OFFICIAL: &str = "KosmosisDire/rant-packages";

/// Undoes every link among the packages, so removing the folder never reaches a linked build.
pub fn unlink_all(home: &Path) {
    for e in std::fs::read_dir(dir(home)).into_iter().flatten().flatten() {
        if is_link(&e.path()) {
            let _ = unlink(&e.path());
        }
    }
}

pub fn dir(home: &Path) -> PathBuf {
    home.join("packages")
}

/// sources.rant, which the user writes.
pub fn sources_file(home: &Path) -> PathBuf {
    home.join("sources.rant")
}

/// What a list of sources named when last read, in a file its key cannot clash in.
fn cache_file(home: &Path, named_by: &str) -> PathBuf {
    home.join("cache").join("sources").join(format!("{}.json", named_by.replace('/', "+")))
}

/// Where one package comes from, and what named it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub name: String,
    /// owner/name, empty for a path.
    pub repo: String,
    /// A local build folder or a .tar.gz archive.
    pub path: PathBuf,
    /// "sources.rant", a database's repo, "link" or "--from".
    pub named_by: String,
    /// Set for a library, which rant add takes instead.
    pub library: Option<package::Library>,
}

/// What the sources named when last read, from the cache, each name from the first list that
/// names it. read lists the sources first when the official database was never read.
pub fn cached(home: &Path, read: bool) -> Vec<Source> {
    if read && !cache_file(home, OFFICIAL).exists() {
        // a broken sources.rant or no network leaves the cache as it is
        if let Ok(mut s) = sources::Sources::new(home) {
            let _ = s.offers("");
        }
    }
    let mut lists = vec!["sources.rant".to_string(), OFFICIAL.to_string()];
    let file = sources_file(home);
    if let Ok(text) = std::fs::read_to_string(&file)
        && let Ok(db) = database::parse(&file, text, true)
    {
        lists.extend(db.databases);
    }
    let mut out: Vec<Source> = Vec::new();
    for l in lists {
        let Ok(Value::Array(entries)) = std::fs::read(cache_file(home, &l)).map(|b| serde_json::from_slice(&b).unwrap_or(Value::Null)) else { continue };
        for e in &entries {
            let Some(name) = e["name"].as_str() else { continue };
            if out.iter().any(|o| o.name == name) {
                continue;
            }
            let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
            let b = &e["library"];
            let library = b.is_object().then(|| package::Library { cmake: text(&b["cmake"]), python: text(&b["python"]), csharp: text(&b["csharp"]), rust: text(&b["rust"]) });
            let path = PathBuf::from(e["path"].as_str().unwrap_or_default());
            out.push(Source { name: name.to_string(), repo: e["repo"].as_str().unwrap_or_default().to_string(), path, named_by: l.clone(), library });
        }
    }
    out
}

/// What rant install recorded of a package it installed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    pub name: String,
    pub repo: String,
    pub path: PathBuf,
    /// Its source's, so update looks where install did.
    pub named_by: String,
    /// The release, empty from a path.
    pub version: String,
    /// The VS Code extension it installed, empty for none.
    pub extension: String,
    /// And the editor and profile that took it.
    pub editor: PathBuf,
    pub profile: String,
    /// The links that give its skill to AI coding agents.
    pub skills: Vec<PathBuf>,
}

/// A JSON file beside each package's folder, since a linked folder is the user's.
fn record_file(home: &Path, name: &str) -> PathBuf {
    dir(home).join(format!("{name}.json"))
}

/// The record of an installed package, None when rant install left none.
pub fn record(home: &Path, name: &str) -> Option<Record> {
    let j: Value = serde_json::from_slice(&std::fs::read(record_file(home, name)).ok()?).ok()?;
    j.is_object().then_some(())?;
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let e = &j["extension"];
    Some(Record {
        name: name.to_string(),
        repo: text(&j["repo"]),
        path: PathBuf::from(text(&j["path"])),
        named_by: text(&j["named_by"]),
        version: text(&j["version"]),
        extension: text(&e["id"]),
        editor: PathBuf::from(text(&e["editor"])),
        profile: text(&e["profile"]),
        skills: j["skills"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect(),
    })
}

/// Writes a package's record beside its folder.
fn save(home: &Path, r: &Record) -> Result<()> {
    let mut j = Map::new();
    let mut put = |k: &str, v: &str| {
        if !v.is_empty() {
            j.insert(k.into(), json!(v));
        }
    };
    put("repo", &r.repo);
    put("path", &paths::slash(&r.path));
    put("named_by", &r.named_by);
    put("version", &r.version);
    if !r.extension.is_empty() {
        j.insert("extension".into(), json!({"id": r.extension, "editor": paths::slash(&r.editor), "profile": r.profile}));
    }
    if !r.skills.is_empty() {
        j.insert("skills".into(), json!(r.skills.iter().map(|l| paths::slash(l)).collect::<Vec<_>>()));
    }
    let file = record_file(home, &r.name);
    std::fs::write(&file, serde_json::to_string_pretty(&Value::Object(j)).unwrap_or_default() + "\n").map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(&file))))
}

/// The packages in rant's home, by folder or record, by name.
pub fn installed(home: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in std::fs::read_dir(dir(home)).into_iter().flatten().flatten() {
        let p = e.path();
        let mut name = e.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        if p.extension().is_some_and(|x| x == "json") {
            name = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        } else if !p.is_dir() {
            continue;
        }
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// A release file downloaded into rant's cache, gone with this. An empty path for no file.
pub struct Download {
    pub path: PathBuf,
}

impl Download {
    pub fn new(ctx: &Context, home: &Path, a: Option<&Asset>) -> Result<Download> {
        let Some(a) = a else { return Ok(Download { path: PathBuf::new() }) };
        ctx.out.note(&format!("downloading {}", a.name));
        let d = Download { path: home.join("cache").join(format!("{}-{}", files::stamp(), a.name)) };
        github::download(&a.url, &d.path, false)?;
        Ok(d)
    }

    /// The file, None for no file.
    pub fn file(&self) -> Option<&Path> {
        (!self.path.as_os_str().is_empty()).then_some(self.path.as_path())
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        if let Some(f) = self.file() {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// A node an installed package declares, as rant's home sees it.
pub fn installed_node(home: &Path, package: &str, node: &str) -> Option<NodeType> {
    let (model, _) = Model::load(Settings::home(home).ok()?, home);
    let p = model.packages.into_iter().find(|p| p.installed && p.name == package)?;
    p.nodes.into_iter().find(|n| n.name == node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn the_cache_and_records_read_back() {
        let t = Scratch::new("packages");
        let home = t.0.clone();
        std::fs::create_dir_all(home.join("cache/sources")).unwrap();
        std::fs::create_dir_all(dir(&home)).unwrap();
        let official = r#"[{"name": "rant", "repo": "KosmosisDire/Rant", "library": {"cmake": "rant::rant_host", "python": "", "csharp": "Rant", "rust": "rant-middleware"}},
            {"name": "explorer", "repo": "KosmosisDire/rant-explorer"}]"#;
        std::fs::write(home.join("cache/sources/KosmosisDire+rant-packages.json"), official).unwrap();
        std::fs::write(home.join("cache/sources/sources.rant.json"), r#"[{"name": "explorer", "path": "/w/explorer"}]"#).unwrap();
        let s = cached(&home, false);
        assert_eq!(s.iter().map(|s| (s.name.as_str(), s.named_by.as_str())).collect::<Vec<_>>(), [("explorer", "sources.rant"), ("rant", OFFICIAL)]);
        let lib = s[1].library.as_ref().unwrap();
        assert_eq!((lib.cmake.as_deref(), lib.python.as_deref()), (Some("rant::rant_host"), None));

        std::fs::write(record_file(&home, "explorer"), r#"{"repo": "KosmosisDire/rant-explorer", "version": "0.0.9", "skills": ["/a/skill"]}"#).unwrap();
        let r = record(&home, "explorer").unwrap();
        assert_eq!((r.version.as_str(), r.skills.len(), r.extension.as_str()), ("0.0.9", 1, ""));
        assert_eq!(record(&home, "nothing"), None);
        assert!(!is_link(&dir(&home)));
    }
}
