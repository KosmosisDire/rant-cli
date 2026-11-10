//! Where packages come from: the lists of config.rant, the official database, then the databases
//! config.rant names, the first naming a package winning. Each list is read once a run and cached.

use std::path::{Path, PathBuf};

use rant_config::database::{self, Database};
use rant_config::home;
use rant_config::package::{self, PACKAGE_FILE, PackageBlock};
use rant_config::paths;
use serde_json::{Map, Value, json};

use super::{OFFICIAL, Record, Source, cache_file, record};
use crate::failure::{Failure, Result, failed};
use crate::github::{self, Asset, Release};
use crate::names;

/// A package a list names, and what its package.rant says it is for.
#[derive(Debug, Clone)]
pub struct Found {
    pub source: Source,
    pub description: String,
}

/// A package there is, from the first list naming it, and its record when installed.
#[derive(Debug, Clone)]
pub struct Offer {
    pub source: Source,
    pub description: String,
    pub record: Option<Record>,
}

struct List {
    named_by: String,
    found: Option<Vec<Found>>,
}

pub struct Sources {
    home: PathBuf,
    local: Database,
    lists: Vec<List>,
    problems: Vec<String>,
}

fn words(e: Failure) -> String {
    e.messages().join("\n")
}

/// Where a source is, as a message names it.
fn shown(s: &Source) -> String {
    if s.repo.is_empty() { paths::slash(&s.path) } else { s.repo.clone() }
}

pub(super) fn is_archive(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".tar.gz"))
}

/// The package an archive holds by its file name, <name>-<version>-<platform>.tar.gz.
fn archive_package(p: &Path) -> String {
    let file = p.file_name().unwrap_or_default().to_string_lossy();
    let stem = file.strip_suffix(".tar.gz").unwrap_or(&file);
    stem.split('-').next().unwrap_or_default().to_string()
}

/// A package.rant, which must name its package, read as dir would hold it. place names it in
/// errors. Returns its name and what it holds.
pub fn named(dir: &Path, place: &str, text: Option<String>) -> Result<(String, PackageBlock)> {
    let Some(text) = text else { return Err(failed(format!("{place} holds no package.rant, so it is no package"))) };
    let rel = package::from_text(dir, text, dir)?;
    let name = rel.name.clone().unwrap_or_default();
    if name.is_empty() {
        return Err(failed(format!("the package.rant of {place} gives no name, which a package needs to install")));
    }
    if !database::is_name(&name) {
        return Err(failed(format!("{place} names its package `{name}`, which is no package name: lowercase letters, digits and _")));
    }
    Ok((name, rel))
}

pub fn read_text(p: &Path) -> Option<String> {
    p.is_file().then(|| std::fs::read_to_string(p).ok()).flatten()
}

/// A package a path names: a folder with a package.rant, or a release archive.
fn path_found(p: &Path, named_by: &str) -> Result<Found> {
    let shown = paths::slash(p);
    if p.is_dir() {
        let (name, rel) = named(p, &shown, read_text(&p.join(PACKAGE_FILE)))?;
        let source = Source { name, repo: String::new(), path: p.to_path_buf(), named_by: named_by.into(), library: rel.library };
        return Ok(Found { source, description: rel.description.unwrap_or_default() });
    }
    if is_archive(p) && p.is_file() {
        let name = archive_package(p);
        if !database::is_name(&name) {
            return Err(failed(format!("{shown} is no archive named <name>-<version>-<platform>.tar.gz")));
        }
        return Ok(Found { source: Source { name, repo: String::new(), path: p.to_path_buf(), named_by: named_by.into(), library: None }, description: String::new() });
    }
    Err(failed(if p.exists() { format!("{shown} is no package folder or .tar.gz archive") } else { format!("no folder or archive {shown}") }))
}

/// A package database's list, fetched from its repo.
fn database_of(repo: &str) -> Result<Database> {
    let Some(text) = github::fetch(&github::raw_url(repo, "HEAD", "packages.rant"))? else {
        return Err(failed(format!("{repo} holds no packages.rant, so it is no package database")));
    };
    Ok(database::parse(Path::new(&format!("{repo}/packages.rant")), text)?)
}

impl Sources {
    /// The lists, none read yet. A broken config.rant fails here.
    pub fn new(home: &Path) -> Result<Sources> {
        let local = home::read(home)?.sources;
        let mut lists = vec![List { named_by: home::CONFIG_FILE.into(), found: None }, List { named_by: OFFICIAL.into(), found: None }];
        lists.extend(local.databases.iter().filter(|d| !d.eq_ignore_ascii_case(OFFICIAL)).map(|d| List { named_by: d.clone(), found: None }));
        Ok(Sources { home: home.to_path_buf(), local, lists, problems: Vec::new() })
    }

    /// What config.rant lists itself: its paths and repos.
    pub fn local(&self) -> &Database {
        &self.local
    }

    /// The databases read after the lists of config.rant, the official one first.
    pub fn databases(&self) -> Vec<String> {
        self.lists[1..].iter().map(|l| l.named_by.clone()).collect()
    }

    /// What reading the lists refused, each a line to warn with.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    fn found(&mut self, i: usize) -> Result<&[Found]> {
        if self.lists[i].found.is_none() {
            let found = self.read(i)?;
            self.lists[i].found = Some(found);
        }
        Ok(self.lists[i].found.as_deref().unwrap_or_default())
    }

    fn read(&mut self, i: usize) -> Result<Vec<Found>> {
        let named_by = self.lists[i].named_by.clone();
        // a database that cannot be read names nothing, so the other lists still count
        let db = if i == 0 {
            self.local.clone()
        } else {
            match database_of(&named_by) {
                Ok(db) => db,
                Err(e) => {
                    self.problems.push(words(e));
                    return Ok(Vec::new());
                }
            }
        };
        let mut out: Vec<Found> = Vec::new();
        let mut add = |f: Found, problems: &mut Vec<String>| match out.iter().find(|o| o.source.name == f.source.name) {
            Some(o) => problems.push(format!("{} also names the package {}, after {}", shown(&f.source), f.source.name, shown(&o.source))),
            None => out.push(f),
        };
        for p in &db.paths {
            match path_found(p, &named_by) {
                Ok(f) => add(f, &mut self.problems),
                Err(e) => self.problems.push(words(e)),
            }
        }
        let urls: Vec<String> = db.repos.iter().map(|r| github::raw_url(r, "HEAD", PACKAGE_FILE)).collect();
        for (repo, text) in db.repos.iter().zip(github::fetch_all(&urls)?) {
            match named(Path::new(repo), repo, text) {
                Ok((name, rel)) => {
                    let source = Source { name, repo: repo.clone(), path: PathBuf::new(), named_by: named_by.clone(), library: rel.library };
                    add(Found { source, description: rel.description.unwrap_or_default() }, &mut self.problems);
                }
                Err(e) => self.problems.push(words(e)),
            }
        }
        save_cache(&self.home, &named_by, &out);
        Ok(out)
    }

    /// The source of a package by name, from the first list that names it.
    pub fn find(&mut self, name: &str) -> Result<Option<Source>> {
        for i in 0..self.lists.len() {
            if let Some(f) = self.found(i)?.iter().find(|f| f.source.name == name) {
                return Ok(Some(f.source.clone()));
            }
        }
        Ok(None)
    }

    /// Every package whose name matches, from the first list naming it.
    pub fn offers(&mut self, pattern: &str) -> Result<Vec<Offer>> {
        let mut out: Vec<Offer> = Vec::new();
        for i in 0..self.lists.len() {
            let found = self.found(i)?.to_vec();
            for f in found {
                if names::matches(pattern, &f.source.name) && !out.iter().any(|o| o.source.name == f.source.name) {
                    let record = record(&self.home, &f.source.name);
                    out.push(Offer { source: f.source, description: f.description, record });
                }
            }
        }
        Ok(out)
    }
}

fn save_cache(home: &Path, named_by: &str, found: &[Found]) {
    let entries: Vec<Value> = found
        .iter()
        .map(|f| {
            let mut j = Map::new();
            j.insert("name".into(), json!(f.source.name));
            if !f.source.repo.is_empty() {
                j.insert("repo".into(), json!(f.source.repo));
            }
            if !f.source.path.as_os_str().is_empty() {
                j.insert("path".into(), json!(paths::slash(&f.source.path)));
            }
            if let Some(b) = &f.source.library {
                j.insert("library".into(), json!({"cmake": b.cmake, "python": b.python, "csharp": b.csharp, "rust": b.rust}));
            }
            Value::Object(j)
        })
        .collect();
    let file = cache_file(home, named_by);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, Value::Array(entries).to_string() + "\n");
}

/// A package by the link of its repo, which names it by its own package.rant.
pub fn from_repo(repo: &str) -> Result<Source> {
    let text = github::fetch(&github::raw_url(repo, "HEAD", PACKAGE_FILE))?;
    let (name, rel) = named(Path::new(repo), repo, text)?;
    Ok(Source { name, repo: repo.into(), path: PathBuf::new(), named_by: "link".into(), library: rel.library })
}

/// The files a release carries for a package, found by their names, and the platforms its
/// programs are built for.
#[derive(Debug, Clone, Default)]
pub struct Carried {
    pub archive: Option<Asset>,
    pub extension: Option<Asset>,
    /// A PLCnext Engineer library.
    pub library: Option<Asset>,
    pub skill: Option<Asset>,
    pub platforms: Vec<String>,
}

pub fn carried(r: &Release, name: &str, platform: &str) -> Carried {
    let prefix = format!("{name}-{}", r.version);
    let platforms = r.assets.iter().filter_map(|a| a.name.strip_prefix(&format!("{prefix}-"))?.strip_suffix(".tar.gz").map(str::to_string));
    Carried {
        archive: r.asset(&format!("{prefix}-{platform}.tar.gz")).cloned(),
        extension: r.asset(&format!("{prefix}.vsix")).cloned(),
        library: r.asset(&format!("{prefix}.pcwlx")).cloned(),
        skill: r.asset(&format!("{prefix}-skill.zip")).cloned(),
        platforms: platforms.collect(),
    }
}

/// What a package not yet installed is: its latest release, the files it carries here, and
/// what its package.rant says it is for.
#[derive(Debug, Clone, Default)]
pub struct About {
    pub version: String,
    pub files: Vec<String>,
    pub description: String,
}

pub fn about(s: &Source) -> Result<About> {
    if s.repo.is_empty() {
        let description = if s.path.is_dir() { named(&s.path, &paths::slash(&s.path), read_text(&s.path.join(PACKAGE_FILE)))?.1.description } else { None };
        return Ok(About { description: description.unwrap_or_default(), ..About::default() });
    }
    let r = github::release(&s.repo, "")?;
    let c = carried(&r, &s.name, github::platform());
    let files = [&c.archive, &c.extension, &c.library, &c.skill].into_iter().flatten().map(|a| a.name.clone()).collect();
    let text = github::fetch(&github::raw_url(&s.repo, &r.tag, PACKAGE_FILE))?;
    let (_, rel) = named(Path::new(&s.repo), &format!("{} at {}", s.repo, r.tag), text)?;
    Ok(About { version: r.version, files, description: rel.description.unwrap_or_default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn config_rant_names_folders_and_archives() {
        let t = Scratch::new("sources-paths");
        t.write("build/package.rant", "name = \"tool\"\ndescription = \"A tool\"\n");
        t.write("lib/package.rant", "name = \"geo\"\nlibrary {\n  cmake = \"geo\"\n}\n");
        t.write("tool-0.1.0-win-x64.tar.gz", "x");
        t.write("Bad-1.tar.gz", "x");
        t.write("nameless/package.rant", "description = \"none\"\n");
        let home = t.path("home");
        t.write("home/config.rant", "paths = [\"../build\", \"../lib\", \"../tool-0.1.0-win-x64.tar.gz\", \"../Bad-1.tar.gz\", \"../nameless\", \"../gone\"]\n");
        let mut s = Sources::new(&home).unwrap();
        let found = s.found(0).unwrap().to_vec();
        assert_eq!(found.iter().map(|f| (f.source.name.as_str(), f.description.as_str())).collect::<Vec<_>>(), [("tool", "A tool"), ("geo", "")]);
        assert!(found[1].source.library.is_some());
        let problems = s.problems().join("\n");
        for p in ["also names the package tool", "Bad-1.tar.gz is no archive named", "nameless gives no name", "no folder or archive"] {
            assert!(problems.contains(p), "{p} in {problems}");
        }
        assert_eq!(s.find("geo").unwrap().map(|s| s.named_by), Some("config.rant".into()));
        let cached = super::super::cached(&home, false);
        assert_eq!(cached.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["tool", "geo"]);
    }

    #[test]
    fn a_broken_config_rant_says_where() {
        let t = Scratch::new("sources-broken");
        t.write("home/config.rant", "paths = [\"\"]\n");
        let e = Sources::new(&t.path("home")).err().unwrap();
        assert!(e.messages()[0].contains("a path must name a folder or an archive"), "{:?}", e.messages());
    }

    #[test]
    fn a_release_carries_files_by_name() {
        let asset = |n: &str| Asset { name: n.into(), url: format!("https://x/{n}") };
        let r = Release { tag: "v0.1.0".into(), version: "0.1.0".into(), assets: ["tool-0.1.0-win-x64.tar.gz", "tool-0.1.0-linux-x64.tar.gz", "tool-0.1.0.vsix", "other-0.1.0-skill.zip"].map(asset).to_vec() };
        let c = carried(&r, "tool", "linux-x64");
        assert_eq!(c.archive.map(|a| a.name).as_deref(), Some("tool-0.1.0-linux-x64.tar.gz"));
        assert!(c.extension.is_some() && c.skill.is_none());
        assert_eq!(c.platforms, ["win-x64", "linux-x64"]);
    }
}
