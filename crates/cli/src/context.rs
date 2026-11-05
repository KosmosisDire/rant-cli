//! What every command gets: the output, the folder rant runs in, rant's home and the
//! workspace around it, read on first use.

use std::path::{Path, PathBuf};

use rant_config::Settings;

use crate::failure::{Result, failed};
use crate::output::{Data, Output};

pub struct Context {
    pub out: Output,
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    settings: Option<Option<Settings>>,
}

impl Context {
    pub fn new(out: Output) -> Context {
        Context { out, cwd: std::env::current_dir().ok(), home: crate::home::rant_home(), settings: None }
    }

    pub fn cwd(&self) -> Result<&Path> {
        self.cwd.as_deref().ok_or_else(|| failed("this folder was deleted, cd elsewhere"))
    }

    /// A command's data, its paths shown from here.
    pub fn show<D: Data>(&self, data: &D) {
        self.out.show(data, self.cwd.as_deref());
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
}
