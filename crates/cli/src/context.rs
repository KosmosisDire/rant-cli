//! What every command gets: the output, the folder rant runs in, rant's home and the
//! workspace around it, read on first use.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use rant_config::{Settings, Workspace};

use crate::failure::{Failure, Result, failed};
use crate::output::{Data, Output};

pub struct Context {
    pub out: Output,
    /// -y: every question is answered yes.
    pub yes: bool,
    /// --domain.
    pub domain_option: Option<u16>,
    cwd: Option<PathBuf>,
    /// Where paths are shown from, which stays when a command enters another folder.
    shown_from: Option<PathBuf>,
    home: Option<PathBuf>,
    settings: Option<Option<Settings>>,
    loaded: Option<Rc<Workspace>>,
}

impl Context {
    pub fn new(out: Output, yes: bool, domain_option: Option<u16>) -> Context {
        let cwd = std::env::current_dir().ok();
        Context { out, yes, domain_option, shown_from: cwd.clone(), cwd, home: crate::home::rant_home(), settings: None, loaded: None }
    }

    pub fn cwd(&self) -> Result<&Path> {
        self.cwd.as_deref().ok_or_else(|| failed("this folder was deleted, cd elsewhere"))
    }

    pub fn home(&self) -> Result<&Path> {
        self.home.as_deref().ok_or_else(|| failed("rant has no home folder, set HOME or RANT_HOME"))
    }

    /// Where paths are shown from: where rant runs.
    pub fn from(&self) -> Option<&Path> {
        self.shown_from.as_deref()
    }

    /// A path for messages, seen from where rant runs.
    pub fn shown(&self, p: &Path) -> String {
        crate::output::shown(p, self.from())
    }

    /// A command's data, its paths shown from where rant runs.
    pub fn show<D: Data>(&self, data: &D) {
        self.out.show(data, self.from());
    }

    pub fn confirm(&self, question: &str) -> bool {
        crate::prompt::confirm(question, self.yes)
    }

    /// Acts from dir from now on, as if rant ran there, so a run another workspace started is
    /// handled through that workspace's config and state. Paths are still shown from here.
    pub fn enter(&mut self, dir: &Path) {
        if self.cwd.as_deref() != Some(dir) {
            (self.cwd, self.settings, self.loaded) = (Some(dir.to_path_buf()), None, None);
        }
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

    /// The domain mesh commands use: --domain, else the workspace's, else RANT_DOMAIN, else 0.
    /// Nodes rant starts take the same order from their config.
    pub fn domain(&mut self) -> Result<u16> {
        if let Some(d) = self.domain_option.or_else(|| self.scope().and_then(|s| s.placement.domain)) {
            return Ok(d);
        }
        match std::env::var("RANT_DOMAIN") {
            Ok(v) if !v.is_empty() => v.parse().map_err(|_| failed(format!("RANT_DOMAIN `{v}` is not a number from 0 to 65535"))),
            _ => Ok(0),
        }
    }

    /// The workspace around here, or rant's home outside every one, with every package and
    /// node type. Any config error is a failure.
    pub fn require_scope(&mut self) -> Result<Rc<Workspace>> {
        if self.loaded.is_none() {
            let (cwd, home) = (self.cwd()?.to_path_buf(), self.home()?.to_path_buf());
            let (ws, diags) = Workspace::load(Settings::find(&cwd, &home)?, &cwd, &home);
            if !diags.is_empty() {
                return Err(Failure::Diags(diags));
            }
            self.loaded = Some(Rc::new(ws));
        }
        Ok(Rc::clone(self.loaded.as_ref().expect("loaded above")))
    }

    /// The workspace around here with every package and node type, or a failure outside one.
    pub fn require_workspace(&mut self) -> Result<Rc<Workspace>> {
        let ws = self.require_scope()?;
        if ws.model.settings.home {
            return Err(failed("no workspace here, `rant init` makes one"));
        }
        Ok(ws)
    }
}
