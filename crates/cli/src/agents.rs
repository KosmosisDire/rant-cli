//! The AI coding agents on this machine, each with the folder it reads skills from.

use std::path::PathBuf;

use crate::home::{env_path, home_dir};

#[derive(Debug, Clone, PartialEq)]
pub struct Agent {
    pub name: &'static str,
    pub skills: PathBuf,
}

/// The agents whose own folder is here. Claude Code reads its own skills folder, the others
/// share ~/.agents/skills.
pub fn detected() -> Vec<Agent> {
    let Some(home) = home_dir() else { return Vec::new() };
    let config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
    let shared = home.join(".agents").join("skills");
    let known = [
        ("Claude Code", home.join(".claude").join("skills"), home.join(".claude")),
        ("Codex", shared.clone(), home.join(".codex")),
        ("Cursor", shared.clone(), home.join(".cursor")),
        ("Gemini CLI", shared.clone(), home.join(".gemini")),
        ("GitHub Copilot", shared.clone(), home.join(".copilot")),
        ("OpenCode", shared, config.join("opencode")),
    ];
    known.into_iter().filter(|(_, _, folder)| folder.is_dir()).map(|(name, skills, _)| Agent { name, skills }).collect()
}
