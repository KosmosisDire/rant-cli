//! The package database, `packages.rant` in the rant-packages repo, and the user's own
//! `sources.rant` in rant's home. A database names each package and the repo it comes from,
//! and the repo's own package.rant says the rest. sources.rant may also name a package's
//! folder or archive on this machine, and more databases to read after the official one.

use std::path::{Path, PathBuf};

use hcl_edit::expr::Expression;

use crate::diag::Diag;
use crate::source::{one_label, Fields, Source};

/// Where one package comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    /// A GitHub repo as owner/name, whose releases carry the package.
    Repo(String),
    /// A local build folder or a release archive on this machine, only in sources.rant.
    Path(PathBuf),
}

/// One package a database or sources.rant names.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub name: String,
    pub origin: Origin,
}

/// What a database file names. Only sources.rant names databases.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Database {
    pub packages: Vec<Listed>,
    /// More databases, as owner/name repos, read in this order after the official one.
    pub databases: Vec<String>,
}

/// Reads a database's text, sources.rant when local. file only names it in errors, and a
/// relative path in sources.rant is from the file's folder.
pub fn parse(file: &Path, text: String, local: bool) -> Result<Database, Diag> {
    let src = Source::parse(file, text)?;
    let what = if local { "sources.rant" } else { "the package database" };
    let blocks: &[&str] = if local { &["package", "database"] } else { &["package"] };
    let fields = Fields::of_body(&src, &src.body, what, &[], blocks)?;
    let mut out = Database::default();
    for block in fields.blocks("package") {
        let name = one_label(&src, block)?;
        if !is_name(name) {
            return Err(src.diag_at(block, format!("package name `{name}` must be lowercase letters, digits and _, starting with a letter")));
        }
        if out.packages.iter().any(|l| l.name == name) {
            return Err(src.diag_at(block, format!("package `{name}` is listed twice")));
        }
        let keys: &[&str] = if local { &["repo", "path"] } else { &["repo"] };
        let f = Fields::of(&src, block, keys, &[])?;
        let origin = match (f.attr("repo"), f.attr("path")) {
            (Some(repo), None) => match &repo.value {
                Expression::String(s) if is_repo(s.value()) => Origin::Repo(s.value().to_string()),
                e => return Err(src.diag_at(e, "`repo` must be a GitHub repo as a plain string, such as \"KosmosisDire/rant-bridge\"")),
            },
            (None, Some(path)) => match &path.value {
                Expression::String(s) if !s.value().is_empty() => Origin::Path(src.dir().join(s.value().as_str())),
                e => return Err(src.diag_at(e, "`path` must be a folder or an archive as a plain string")),
            },
            (Some(_), Some(_)) => return Err(src.diag_at(block, format!("package `{name}` takes `repo` or `path`, not both"))),
            (None, None) if local => return Err(src.diag_at(block, format!("package `{name}` needs `repo` or `path`"))),
            (None, None) => return Err(src.diag_at(block, format!("package `{name}` needs `repo`"))),
        };
        out.packages.push(Listed { name: name.to_string(), origin });
    }
    for block in fields.blocks("database") {
        let repo = one_label(&src, block)?;
        if !is_repo(repo) {
            return Err(src.diag_at(block, format!("database `{repo}` must be a GitHub repo, such as \"someone/rant-packages\"")));
        }
        if out.databases.iter().any(|d| d == repo) {
            return Err(src.diag_at(block, format!("database `{repo}` is listed twice")));
        }
        Fields::of(&src, block, &[], &[])?;
        out.databases.push(repo.to_string());
    }
    Ok(out)
}

/// A package name the database can hold, also the folder it installs into.
fn is_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
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
    fn reads_each_package_and_its_repo() {
        let db = read("package \"bridge\" {\n  repo = \"KosmosisDire/rant-bridge\"\n}\npackage \"media_transcoder\" {\n  repo = \"a/b.c\"\n}\n").unwrap();
        assert_eq!(
            db.packages,
            [
                Listed { name: "bridge".into(), origin: Origin::Repo("KosmosisDire/rant-bridge".into()) },
                Listed { name: "media_transcoder".into(), origin: Origin::Repo("a/b.c".into()) },
            ]
        );
        assert!(db.databases.is_empty());
    }

    #[test]
    fn rejects_what_it_cannot_trust() {
        let cases = [
            ("package \"Bridge\" {\n  repo = \"a/b\"\n}\n", "lowercase letters"),
            ("package \"x\" {\n  repo = \"a/b\"\n}\npackage \"x\" {\n  repo = \"a/c\"\n}\n", "listed twice"),
            ("package \"x\" {}\n", "needs `repo`"),
            ("package \"x\" {\n  repo = \"https://github.com/a/b\"\n}\n", "GitHub repo"),
            ("package \"x\" {\n  repo = \"a/..\"\n}\n", "GitHub repo"),
            ("package \"x\" {\n  repo = \"${x}/b\"\n}\n", "plain string"),
            ("package \"x\" {\n  repo = \"a/b\"\n  description = \"d\"\n}\n", "unknown attribute `description`"),
            ("package \"x\" {\n  path = \"/x\"\n}\n", "unknown attribute `path`"),
            ("database \"a/b\" {}\n", "unknown block `database`"),
            ("repo = \"a/b\"\n", "unknown attribute `repo`"),
        ];
        for (text, message) in cases {
            let err = read(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }

    #[test]
    fn sources_name_folders_and_more_databases() {
        let db = sources(
            "package \"explorer\" {\n  path = \"builds/explorer\"\n}\npackage \"tool\" {\n  repo = \"me/tool\"\n}\n\
             database \"me/packages\" {}\ndatabase \"team/packages\" {}\n",
        )
        .unwrap();
        assert_eq!(
            db.packages,
            [
                Listed { name: "explorer".into(), origin: Origin::Path(Path::new("home").join("builds/explorer")) },
                Listed { name: "tool".into(), origin: Origin::Repo("me/tool".into()) },
            ]
        );
        assert_eq!(db.databases, ["me/packages", "team/packages"], "in the order written");
        let absolute = if cfg!(windows) { "C:/builds/explorer" } else { "/builds/explorer" };
        let db = sources(&format!("package \"explorer\" {{\n  path = \"{absolute}\"\n}}\n")).unwrap();
        assert_eq!(db.packages[0].origin, Origin::Path(PathBuf::from(absolute)));
    }

    #[test]
    fn sources_refuse_what_they_cannot_use() {
        let cases = [
            ("package \"x\" {}\n", "needs `repo` or `path`"),
            ("package \"x\" {\n  repo = \"a/b\"\n  path = \"c\"\n}\n", "not both"),
            ("package \"x\" {\n  path = \"\"\n}\n", "`path` must be"),
            ("database \"nope\" {}\n", "must be a GitHub repo"),
            ("database \"a/b\" {}\ndatabase \"a/b\" {}\n", "listed twice"),
            ("database \"a/b\" {\n  repo = \"c/d\"\n}\n", "unknown attribute `repo`"),
        ];
        for (text, message) in cases {
            let err = sources(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }
}
