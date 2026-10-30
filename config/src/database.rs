//! The package database, `packages.rant` in the rant-packages repo, and the user's own
//! `sources.rant` in rant's home. A database lists GitHub repos, and the package.rant at each
//! repo's root names its package and says the rest. sources.rant may also list folders and
//! archives on this machine, and more databases to read after the official one.

use std::path::{Path, PathBuf};

use hcl_edit::expr::Expression;

use crate::diag::Diag;
use crate::paths;
use crate::source::{Fields, Source};

/// What a database file lists. Only sources.rant lists paths and databases.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Database {
    /// GitHub repos as owner/name, each releasing one package.
    pub repos: Vec<String>,
    /// Local build folders and release archives, from the file's folder when relative.
    pub paths: Vec<PathBuf>,
    /// More databases as owner/name, read in this order after the official one.
    pub databases: Vec<String>,
}

/// Reads a database's text, sources.rant when local. file only names it in errors, and a
/// relative path in sources.rant is from the file's folder.
pub fn parse(file: &Path, text: String, local: bool) -> Result<Database, Diag> {
    let src = Source::parse(file, text)?;
    let what = if local { "sources.rant" } else { "the package database" };
    let keys: &[&str] = if local { &["repos", "paths", "databases"] } else { &["repos"] };
    let fields = Fields::of_body(&src, &src.body, what, keys, &[])?;
    let mut out = Database { repos: repo_list(&src, &fields, "repos")?, databases: repo_list(&src, &fields, "databases")?, ..Default::default() };
    for (s, at) in plain_list(&src, &fields, "paths")? {
        if s.is_empty() {
            return Err(src.diag_at(at, "a path must name a folder or an archive"));
        }
        let path = paths::normalize(&src.dir().join(s));
        if out.paths.iter().any(|p| paths::same(p, &path)) {
            return Err(src.diag_at(at, format!("{s} is listed twice")));
        }
        out.paths.push(path);
    }
    Ok(out)
}

fn repo_list(src: &Source, fields: &Fields, key: &str) -> Result<Vec<String>, Diag> {
    let mut out: Vec<String> = Vec::new();
    for (s, at) in plain_list(src, fields, key)? {
        let Some(repo) = repo_of(s) else {
            return Err(src.diag_at(at, format!("`{s}` is no GitHub repo, such as \"KosmosisDire/rant-bridge\" or its link")));
        };
        if out.iter().any(|r| r.eq_ignore_ascii_case(&repo)) {
            return Err(src.diag_at(at, format!("{repo} is listed twice")));
        }
        out.push(repo);
    }
    Ok(out)
}

/// The plain strings a list holds, nothing computed, since a database is read as data.
fn plain_list<'a>(src: &Source, fields: &Fields<'a>, key: &str) -> Result<Vec<(&'a str, &'a Expression)>, Diag> {
    let Some(attr) = fields.attr(key) else { return Ok(Vec::new()) };
    let Expression::Array(items) = &attr.value else {
        return Err(src.diag_at(&attr.value, format!("`{key}` must be a list of plain strings")));
    };
    items
        .iter()
        .map(|e| match e {
            Expression::String(s) => Ok((s.value().as_str(), e)),
            e => Err(src.diag_at(e, format!("`{key}` must hold plain strings"))),
        })
        .collect()
}

/// A package name the database can hold, also the folder it installs into.
pub fn is_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The GitHub repo a word names, as owner/name: owner/name itself or a link into the repo.
pub fn repo_of(word: &str) -> Option<String> {
    let scheme = ["https://", "http://"].iter().find_map(|p| word.strip_prefix(p));
    let link = scheme.unwrap_or(word);
    let path = link.strip_prefix("www.github.com/").or_else(|| link.strip_prefix("github.com/"));
    let repo = match path {
        Some(path) => {
            let mut parts = path.split('/').filter(|p| !p.is_empty());
            let (owner, name) = (parts.next()?, parts.next()?);
            format!("{owner}/{}", name.strip_suffix(".git").unwrap_or(name))
        }
        None if scheme.is_none() => word.strip_suffix(".git").unwrap_or(word).to_string(),
        None => return None,
    };
    is_repo(&repo).then_some(repo)
}

fn is_repo(repo: &str) -> bool {
    let part = |s: &str| !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    repo.split_once('/').is_some_and(|(owner, name)| part(owner) && part(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Result<Database, Diag> {
        parse(Path::new("packages.rant"), text.to_string(), false)
    }

    fn sources(text: &str) -> Result<Database, Diag> {
        parse(Path::new("home/sources.rant"), text.to_string(), true)
    }

    #[test]
    fn reads_each_repo() {
        let db = read("repos = [\n  \"KosmosisDire/rant-bridge\",\n  \"a/b.c\",\n]\n").unwrap();
        assert_eq!(db.repos, ["KosmosisDire/rant-bridge", "a/b.c"]);
        assert!(db.paths.is_empty() && db.databases.is_empty());
        assert_eq!(read("").unwrap(), Database::default());
    }

    #[test]
    fn takes_a_repo_or_its_link() {
        for word in [
            "a/b",
            "a/b.git",
            "https://github.com/a/b",
            "https://github.com/a/b/",
            "https://github.com/a/b.git",
            "http://www.github.com/a/b",
            "github.com/a/b",
            "https://github.com/a/b/releases/tag/v1.0",
        ] {
            assert_eq!(repo_of(word).as_deref(), Some("a/b"), "{word}");
        }
        for word in ["a", "a/", "/b", "a/b/c", "a/..", "https://gitlab.com/a/b", "https://github.com/a", "bridge", "a b/c"] {
            assert_eq!(repo_of(word), None, "{word}");
        }
    }

    #[test]
    fn rejects_what_it_cannot_trust() {
        let cases = [
            ("repos = [\"a/b\", \"A/B\"]\n", "listed twice"),
            ("repos = [\"bridge\"]\n", "is no GitHub repo"),
            ("repos = [\"a/..\"]\n", "is no GitHub repo"),
            ("repos = [\"${x}/b\"]\n", "plain strings"),
            ("repos = \"a/b\"\n", "list of plain strings"),
            ("repos = [1]\n", "plain strings"),
            ("paths = [\"x\"]\n", "unknown attribute `paths`"),
            ("databases = [\"a/b\"]\n", "unknown attribute `databases`"),
            ("package \"x\" {\n  repo = \"a/b\"\n}\n", "unknown block `package`"),
        ];
        for (text, message) in cases {
            let err = read(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }

    #[test]
    fn sources_list_repos_paths_and_more_databases() {
        let db = sources(
            "repos     = [\"https://github.com/me/tool\"]\npaths     = [\"builds/explorer\", \"tool-1.0-win-x64.tar.gz\"]\n\
             databases = [\"me/packages\", \"team/packages\"]\n",
        )
        .unwrap();
        assert_eq!(db.repos, ["me/tool"]);
        assert_eq!(db.paths, [paths::normalize(Path::new("home/builds/explorer")), paths::normalize(Path::new("home/tool-1.0-win-x64.tar.gz"))]);
        assert_eq!(db.databases, ["me/packages", "team/packages"], "in the order written");
        let absolute = if cfg!(windows) { "C:/builds/explorer" } else { "/builds/explorer" };
        let db = sources(&format!("paths = [\"{absolute}\"]\n")).unwrap();
        assert_eq!(db.paths, [PathBuf::from(absolute)]);
    }

    #[test]
    fn sources_refuse_what_they_cannot_use() {
        let cases = [
            ("paths = [\"\"]\n", "must name a folder or an archive"),
            ("paths = [\"a\", \"./a\"]\n", "listed twice"),
            ("databases = [\"nope\"]\n", "is no GitHub repo"),
            ("databases = [\"a/b\", \"a/b\"]\n", "listed twice"),
            ("database \"a/b\" {}\n", "unknown block `database`"),
            ("repo = \"c/d\"\n", "unknown attribute `repo`"),
        ];
        for (text, message) in cases {
            let err = sources(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }
}
