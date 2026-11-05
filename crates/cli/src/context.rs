//! What every command gets: the output, the folder rant runs in, rant's home and the
//! workspace around it, read on first use.

use std::path::{Path, PathBuf};

use rant_config::{Settings, Workspace};

use crate::failure::{Failure, Result, failed};
use crate::output::{Data, Output};

pub struct Context {
    pub out: Output,
    /// -y: every question is answered yes.
    pub yes: bool,
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    settings: Option<Option<Settings>>,
    loaded: Option<Workspace>,
}

impl Context {
    pub fn new(out: Output, yes: bool) -> Context {
        let cwd = std::env::current_dir().ok();
        Context { out, yes, cwd, home: crate::home::rant_home(), settings: None, loaded: None }
    }

    pub fn cwd(&self) -> Result<&Path> {
        self.cwd.as_deref().ok_or_else(|| failed("this folder was deleted, cd elsewhere"))
    }

    pub fn home(&self) -> Result<&Path> {
        self.home.as_deref().ok_or_else(|| failed("rant has no home folder, set HOME or RANT_HOME"))
    }

    /// A command's data, its paths shown from here.
    pub fn show<D: Data>(&self, data: &D) {
        self.out.show(data, self.cwd.as_deref());
    }

    pub fn confirm(&self, question: &str) -> bool {
        crate::prompt::confirm(question, self.yes)
    }

    /// The workspace around here, or rant's home outside every one. A broken config is a
    /// warning, since a command that needs none still works.
    pub fn scope(&mut self) -> Option<&Settings> {
        if self.settings.is_none() {
            // a deleted folder holds no workspace, and a command that needs it fails on cwd
            let found = match (self.cwd.as_deref(), &self.home) {
                (None, _) => Ok(None),
                (Some(cwd), Some(home)) => Settings::find(cwd, home).map(Some),
                (Some(cwd), None) => Settings::open(cwd),
            };
            let found = found.unwrap_or_else(|d| {
                self.out.warn(&d.to_string());
                None
            });
            self.settings = Some(found);
        }
        self.settings.as_ref().and_then(Option::as_ref)
    }

    /// The workspace around here, None outside every one.
    pub fn workspace(&mut self) -> Option<&Settings> {
        self.scope().filter(|s| !s.home)
    }

    /// The workspace around here, or rant's home outside every one, with every package and
    /// node type. Any config error is a failure.
    pub fn require_scope(&mut self) -> Result<&Workspace> {
        if self.loaded.is_none() {
            let (cwd, home) = (self.cwd()?.to_path_buf(), self.home()?.to_path_buf());
            let (ws, diags) = Workspace::load(Settings::find(&cwd, &home)?, &cwd, &home);
            if !diags.is_empty() {
                return Err(Failure::Diags(diags));
            }
            self.loaded = Some(ws);
        }
        Ok(self.loaded.as_ref().expect("loaded above"))
    }

    /// The workspace around here with every package and node type, or a failure outside one.
    pub fn require_workspace(&mut self) -> Result<&Workspace> {
        let ws = self.require_scope()?;
        if ws.model.settings.home {
            return Err(failed("no workspace here, `rant init` makes one"));
        }
        Ok(ws)
    }
}
