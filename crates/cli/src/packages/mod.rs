//! The packages rant install puts in the home's packages folder, each from a repo's releases or
//! a path that sources.rant or a package database names, with a record beside it.

use std::path::{Path, PathBuf};

use rant_config::model::{Model, NodeType};
use rant_config::{Settings, database, package};
use serde_json::Value;

/// The official package database, read after sources.rant.
pub const OFFICIAL: &str = "KosmosisDire/rant-packages";

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
/// names it.
pub fn cached(home: &Path) -> Vec<Source> {
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
            let library = e["library"].as_object().map(|b| package::Library { cmake: text(&b["cmake"]), python: text(&b["python"]), csharp: text(&b["csharp"]), rust: text(&b["rust"]) });
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

/// A node an installed package declares, as rant's home sees it.
pub fn installed_node(home: &Path, package: &str, node: &str) -> Option<NodeType> {
    let (model, _) = Model::load(Settings::home(home).ok()?, home);
    let p = model.packages.into_iter().find(|p| p.installed && p.name == package)?;
    p.nodes.into_iter().find(|n| n.name == node)
}

/// Whether p is a symbolic link or, on Windows, a junction, as a package from a local build
/// folder is.
pub fn is_link(p: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_and_records_read_back() {
        let home = std::env::temp_dir().join(format!("rant-unit-packages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("cache/sources")).unwrap();
        std::fs::create_dir_all(dir(&home)).unwrap();
        let official = r#"[{"name": "rant", "repo": "KosmosisDire/Rant", "library": {"cmake": "rant::rant_host", "python": "", "csharp": "Rant", "rust": "rant-middleware"}},
            {"name": "explorer", "repo": "KosmosisDire/rant-explorer"}]"#;
        std::fs::write(home.join("cache/sources/KosmosisDire+rant-packages.json"), official).unwrap();
        std::fs::write(home.join("cache/sources/sources.rant.json"), r#"[{"name": "explorer", "path": "/w/explorer"}]"#).unwrap();
        let s = cached(&home);
        assert_eq!(s.iter().map(|s| (s.name.as_str(), s.named_by.as_str())).collect::<Vec<_>>(), [("explorer", "sources.rant"), ("rant", OFFICIAL)]);
        let lib = s[1].library.as_ref().unwrap();
        assert_eq!((lib.cmake.as_deref(), lib.python.as_deref()), (Some("rant::rant_host"), None));

        std::fs::write(record_file(&home, "explorer"), r#"{"repo": "KosmosisDire/rant-explorer", "version": "0.0.9", "skills": ["/a/skill"]}"#).unwrap();
        let r = record(&home, "explorer").unwrap();
        assert_eq!((r.version.as_str(), r.skills.len(), r.extension.as_str()), ("0.0.9", 1, ""));
        assert_eq!(record(&home, "nothing"), None);
        assert!(!is_link(&dir(&home)));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
