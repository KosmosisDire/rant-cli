//! config.rant in rant's home, which the user writes: where rant install finds packages, and
//! whether the skills packages and libraries carry are given to AI coding agents unasked.

use std::path::{Path, PathBuf};

use hcl_edit::Span;
use hcl_edit::expr::Expression;

use crate::database::{self, Database};
use crate::diag::Diag;
use crate::source::{Fields, Source};

pub const CONFIG_FILE: &str = "config.rant";

const KEYS: [&str; 4] = ["repos", "paths", "databases", "agent_skills"];

/// Whether a skill a package or a library carries is given to the agents found unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSkills {
    Ask,
    Auto,
}

impl AgentSkills {
    fn word(self) -> &'static str {
        match self {
            AgentSkills::Ask => "ask",
            AgentSkills::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HomeConfig {
    /// The paths and repos it lists, and the databases read after the official one.
    pub sources: Database,
    /// None until set.
    pub agent_skills: Option<AgentSkills>,
}

pub fn file(home: &Path) -> PathBuf {
    home.join(CONFIG_FILE)
}

/// The config of rant's home, the defaults when the file is not there.
pub fn read(home: &Path) -> Result<HomeConfig, Diag> {
    let file = file(home);
    match existing(&file)? {
        Some(text) => parse(&file, text),
        None => Ok(HomeConfig::default()),
    }
}

/// Reads config.rant's text. file names it in errors, and a relative path is from its folder.
pub fn parse(file: &Path, text: String) -> Result<HomeConfig, Diag> {
    let src = Source::parse(file, text)?;
    let fields = Fields::of_body(&src, &src.body, CONFIG_FILE, &KEYS, &[])?;
    let agent_skills = match fields.attr("agent_skills").map(|a| &a.value) {
        None => None,
        Some(Expression::String(s)) if s.value() == "auto" => Some(AgentSkills::Auto),
        Some(Expression::String(s)) if s.value() == "ask" => Some(AgentSkills::Ask),
        Some(v) => return Err(src.diag_at(v, "`agent_skills` is \"auto\" or \"ask\"")),
    };
    Ok(HomeConfig { sources: database::lists(&src, &fields)?, agent_skills })
}

/// Sets agent_skills, keeping the rest of the file as written.
pub fn set_agent_skills(home: &Path, value: AgentSkills) -> Result<(), Diag> {
    let file = file(home);
    let src = Source::parse(&file, existing(&file)?.unwrap_or_default())?;
    let fields = Fields::of_body(&src, &src.body, CONFIG_FILE, &KEYS, &[])?;
    let word = format!("\"{}\"", value.word());
    let mut text = src.text.clone();
    match fields.attr("agent_skills").and_then(|a| a.value.span()) {
        Some(at) => text.replace_range(at, &word),
        None => {
            let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
            if !text.is_empty() && !text.ends_with('\n') {
                text.push_str(eol);
            }
            text += &format!("agent_skills = {word}{eol}");
        }
    }
    std::fs::create_dir_all(home).and_then(|_| std::fs::write(&file, text)).map_err(|e| Diag::file(&file, format!("cannot write: {e}")))
}

fn existing(file: &Path) -> Result<Option<String>, Diag> {
    match std::fs::read_to_string(file) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Diag::file(file, format!("cannot read: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::testdir::TestDir;

    fn config(text: &str) -> Result<HomeConfig, Diag> {
        parse(Path::new("home/config.rant"), text.to_string())
    }

    #[test]
    fn lists_repos_paths_and_more_databases() {
        let c = config(
            "repos     = [\"https://github.com/me/tool\"]\npaths     = [\"builds/explorer\", \"tool-1.0-win-x64.tar.gz\"]\n\
             databases = [\"me/packages\", \"team/packages\"]\n",
        )
        .unwrap();
        assert_eq!(c.sources.repos, ["me/tool"]);
        assert_eq!(c.sources.paths, [paths::normalize(Path::new("home/builds/explorer")), paths::normalize(Path::new("home/tool-1.0-win-x64.tar.gz"))]);
        assert_eq!(c.sources.databases, ["me/packages", "team/packages"], "in the order written");
        let absolute = if cfg!(windows) { "C:/builds/explorer" } else { "/builds/explorer" };
        assert_eq!(config(&format!("paths = [\"{absolute}\"]\n")).unwrap().sources.paths, [PathBuf::from(absolute)]);
        assert_eq!(config("").unwrap(), HomeConfig::default());
    }

    #[test]
    fn refuses_what_it_cannot_use() {
        let cases = [
            ("paths = [\"\"]\n", "must name a folder or an archive"),
            ("paths = [\"a\", \"./a\"]\n", "listed twice"),
            ("databases = [\"nope\"]\n", "is no GitHub repo"),
            ("databases = [\"a/b\", \"a/b\"]\n", "listed twice"),
            ("database \"a/b\" {}\n", "unknown block `database`"),
            ("repo = \"c/d\"\n", "unknown attribute `repo`"),
            ("agent_skills = \"yes\"\n", "is \"auto\" or \"ask\""),
            ("agent_skills = true\n", "is \"auto\" or \"ask\""),
        ];
        for (text, message) in cases {
            let err = config(text).err().unwrap_or_else(|| panic!("{text} was read"));
            assert!(err.message.contains(message), "{text}: {}", err.message);
        }
    }

    #[test]
    fn agent_skills_is_set_keeping_the_file() {
        let t = TestDir::new();
        let home = t.path("home");
        assert_eq!(read(&home).unwrap().agent_skills, None);
        set_agent_skills(&home, AgentSkills::Auto).unwrap();
        assert_eq!(std::fs::read_to_string(file(&home)).unwrap(), "agent_skills = \"auto\"\n");
        t.write("home/config.rant", "# mine\nrepos = [\"me/tool\"]\nagent_skills = \"auto\" # why\n");
        set_agent_skills(&home, AgentSkills::Ask).unwrap();
        assert_eq!(std::fs::read_to_string(file(&home)).unwrap(), "# mine\nrepos = [\"me/tool\"]\nagent_skills = \"ask\" # why\n");
        t.write("home/config.rant", "repos = [\"me/tool\"]");
        set_agent_skills(&home, AgentSkills::Auto).unwrap();
        let c = read(&home).unwrap();
        assert_eq!((c.sources.repos, c.agent_skills), (vec!["me/tool".to_string()], Some(AgentSkills::Auto)));
    }
}
