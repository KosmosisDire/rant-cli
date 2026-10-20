//! The package database, `packages.rant` in the rant-packages repo. It names each package
//! and the repo it comes from, and the repo's own package.rant says the rest.

use std::path::Path;

use hcl_edit::expr::Expression;

use crate::diag::Diag;
use crate::source::{one_label, Fields, Source};

/// One package the database names.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub name: String,
    /// A GitHub repo as owner/name.
    pub repo: String,
}

/// Reads the database's text. file only names it in errors.
pub fn parse(file: &Path, text: String) -> Result<Vec<Listed>, Diag> {
    let src = Source::parse(file, text)?;
    let fields = Fields::of_body(&src, &src.body, "the package database", &[], &["package"])?;
    let mut out: Vec<Listed> = Vec::new();
    for block in fields.blocks("package") {
        let name = one_label(&src, block)?;
        if !is_name(name) {
            return Err(src.diag_at(block, format!("package name `{name}` must be lowercase letters, digits and _, starting with a letter")));
        }
        if out.iter().any(|l| l.name == name) {
            return Err(src.diag_at(block, format!("package `{name}` is listed twice")));
        }
        let f = Fields::of(&src, block, &["repo"], &[])?;
        let Some(repo) = f.attr("repo") else {
            return Err(src.diag_at(block, format!("package `{name}` needs `repo`")));
        };
        let value = match &repo.value {
            Expression::String(s) if is_repo(s.value()) => s.value().to_string(),
            e => return Err(src.diag_at(e, "`repo` must be a GitHub repo as a plain string, such as \"KosmosisDire/rant-bridge\"")),
        };
        out.push(Listed { name: name.to_string(), repo: value });
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

    fn read(text: &str) -> Result<Vec<Listed>, Diag> {
        parse(Path::new("packages.rant"), text.to_string())
    }

    #[test]
    fn reads_each_package_and_its_repo() {
        let listed = read("package \"bridge\" {\n  repo = \"KosmosisDire/rant-bridge\"\n}\npackage \"media_transcoder\" {\n  repo = \"a/b.c\"\n}\n").unwrap();
        assert_eq!(
            listed,
            [
                Listed { name: "bridge".into(), repo: "KosmosisDire/rant-bridge".into() },
                Listed { name: "media_transcoder".into(), repo: "a/b.c".into() },
            ]
        );
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
            ("repo = \"a/b\"\n", "unknown attribute `repo`"),
        ];
        for (text, message) in cases {
            let err = read(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }
}
