//! The record of what rant started: the roots asked for and the node instances behind them.
//! Every command reads it, checks it against the OS, acts and writes it back under one lock.

use std::collections::BTreeMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};

use rant_config::plan::Restart;
use rant_config::refs::same_name;
use rant_process::Tracking;
use serde::{Deserialize, Serialize};

use crate::failure::{Result, failed};
use crate::files::{path_key, write_atomic};

const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootKind {
    Group,
    Node,
}

/// Something started on purpose: a group with its params, or one node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Root {
    pub kind: RootKind,
    /// The group in full, or the node instance.
    pub name: String,
    /// A group's resolved params, as text.
    pub params: BTreeMap<String, String>,
}

impl Root {
    pub fn group(name: &str, params: BTreeMap<String, String>) -> Root {
        Root { kind: RootKind::Group, name: name.to_string(), params }
    }

    /// The root of a group run with these resolved params.
    pub fn planned(group: &str, values: &BTreeMap<String, rant_config::Value>) -> Root {
        Root::group(group, values.iter().map(|(k, v)| (k.clone(), rant_config::param::text(v))).collect())
    }

    pub fn node(name: &str) -> Root {
        Root { kind: RootKind::Node, name: name.to_string(), params: BTreeMap::new() }
    }

    /// "group nav a=1 target=red" or "node talker": one per root.
    pub fn key(&self) -> String {
        let kind = match self.kind {
            RootKind::Group => "group",
            RootKind::Node => "node",
        };
        self.params.iter().fold(format!("{kind} {}", self.name), |k, (p, v)| format!("{k} {p}={v}"))
    }

    /// Its params as `key=value` words, as a run is given them.
    pub fn words(&self) -> Vec<String> {
        self.params.iter().map(|(k, v)| format!("{k}={v}")).collect()
    }
}

/// The group a root key names, None for a node's own root.
pub fn group_of_key(key: &str) -> Option<&str> {
    key.strip_prefix("group ").map(|rest| rest.split(' ').next().unwrap_or(rest))
}

/// One running node and how to find it again.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    /// Its name on the mesh.
    pub name: String,
    /// The node type, package/name.
    #[serde(rename = "type")]
    pub node_type: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: String,
    pub log: String,
    pub pid: u64,
    pub start_time: u64,
    pub job: String,
    /// Keys of the roots that need it, in order.
    pub roots: Vec<String>,
    /// The group whose own node block made it, empty for a node run alone.
    #[serde(default)]
    pub group: String,
    /// False for a program that never joins the mesh.
    #[serde(default = "yes")]
    pub expect_mesh: bool,
    /// What the service process does once it exits.
    #[serde(default)]
    pub restart: Restart,
    /// How often the service process started it again.
    #[serde(default)]
    pub restarts: u32,
    /// The environment of the command that left it to the service process.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_env: Vec<String>,
    /// The Windows session it runs in, 0 elsewhere.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub session: u32,
}

fn yes() -> bool {
    true
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Instance {
    /// Whether its working folder or a path in its command line lies in folder.
    pub fn runs_from(&self, folder: &Path) -> bool {
        std::iter::once(&self.cwd).chain(&self.argv).any(|p| rant_config::paths::within(Path::new(p), folder))
    }

    pub fn tracking(&self) -> Tracking {
        Tracking { pid: self.pid, start_time: self.start_time, job: self.job.clone() }
    }

    pub fn track(&mut self, t: &Tracking) {
        (self.pid, self.start_time, self.job) = (t.pid, t.start_time, t.job.clone());
    }

    /// Whether a group's root holds it. Its own start does not count, so the last group
    /// holding a node stops it.
    pub fn held_by_group(&self) -> bool {
        self.roots.iter().any(|k| group_of_key(k).is_some())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    format: u32,
    pub roots: Vec<Root>,
    /// In start order.
    pub instances: Vec<Instance>,
}

impl State {
    /// The node of that name, matched as same_name does when no name is exact.
    pub fn instance(&self, name: &str) -> Option<&Instance> {
        self.instances.iter().find(|i| i.name == name).or_else(|| self.instances.iter().find(|i| same_name(&i.name, name)))
    }

    pub fn instance_mut(&mut self, name: &str) -> Option<&mut Instance> {
        let at = self.instances.iter().position(|i| i.name == name).or_else(|| self.instances.iter().position(|i| same_name(&i.name, name)))?;
        Some(&mut self.instances[at])
    }

    pub fn root(&self, key: &str) -> Option<&Root> {
        self.roots.iter().find(|r| r.key() == key)
    }

    /// Drops a stopped instance, with the node root that started it.
    pub fn forget(&mut self, name: &str) {
        self.instances.retain(|i| i.name != name);
        self.roots.retain(|r| !(r.kind == RootKind::Node && r.name == name));
    }

    /// The nodes a group's own node blocks made, in start order, whichever run started them.
    pub fn made_by(&self, group: &str) -> Vec<String> {
        self.instances.iter().filter(|i| i.group == group).map(|i| i.name.clone()).collect()
    }
}

/// The state file, locked while the Store lives so two commands never act on it at once.
pub struct Store {
    path: PathBuf,
    lock: Option<File>,
    state: State,
}

impl Store {
    /// Waits for the lock, unless wait is false: then a busy state gives an empty Store whose
    /// locked() is false.
    pub fn open(data: &Path, wait: bool) -> Result<Store> {
        std::fs::create_dir_all(data).map_err(|e| failed(format!("cannot make {}: {e}", data.display())))?;
        let lock_path = data.join("state.lock");
        // std opens it uninheritable, so no node ever holds the lock
        let file = File::options().read(true).write(true).create(true).truncate(false).open(&lock_path);
        let file = file.map_err(|e| failed(format!("cannot open {}: {e}", lock_path.display())))?;
        let held = if wait { file.lock().map_err(TryLockError::Error) } else { file.try_lock() };
        let path = data.join("state");
        match held {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(Store { path, lock: None, state: State::default() }),
            Err(TryLockError::Error(e)) => return Err(failed(format!("cannot lock {}: {e}", lock_path.display()))),
        }
        let state = match std::fs::read(&path) {
            Ok(bytes) => parse(&bytes).ok_or_else(|| failed(format!("{} is damaged, delete it to start over (running nodes are then forgotten)", path.display())))?,
            Err(_) => State::default(),
        };
        Ok(Store { path, lock: Some(file), state })
    }

    pub fn locked(&self) -> bool {
        self.lock.is_some()
    }

    pub fn state(&mut self) -> &mut State {
        &mut self.state
    }

    pub fn save(&mut self) -> Result<()> {
        self.state.format = FORMAT;
        let text = serde_json::to_string_pretty(&self.state).expect("a state always serializes") + "\n";
        write_atomic(&self.path, &text).map_err(|e| failed(format!("cannot write {}: {e}", self.path.display())))
    }

    pub fn into_state(self) -> State {
        self.state
    }
}

/// A state of another format is an empty one, never an error.
fn parse(bytes: &[u8]) -> Option<State> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if v.get("format").and_then(serde_json::Value::as_u64) != Some(FORMAT as u64) {
        return Some(State::default());
    }
    serde_json::from_value(v).ok()
}

/// Drops the instances whose processes are gone and their node roots, keeping group roots and
/// what a running service process restarts. Gives the names dropped.
pub fn prune(s: &mut State, restarting: impl Fn(u32) -> bool) -> Vec<String> {
    let mut dropped = Vec::new();
    s.instances.retain(|i| {
        let left = i.restart != Restart::Never || i.pid == 0;
        let keep = rant_process::alive(&i.tracking()) || (left && restarting(i.session));
        if !keep {
            dropped.push(i.name.clone());
        }
        keep
    });
    let names: Vec<String> = s.instances.iter().map(|i| i.name.clone()).collect();
    s.roots.retain(|r| r.kind == RootKind::Group || names.iter().any(|n| n == &r.name || same_name(n, &r.name)));
    dropped
}

/// Every workspace's data folder rant started a node from, kept in the home's workspaces
/// folder so a stop anywhere on the machine finds them.
pub fn remember(home: &Path, data: &Path) {
    let entry = home.join("workspaces").join(path_key(data));
    if !entry.exists() {
        let _ = std::fs::create_dir_all(home.join("workspaces"));
        let _ = std::fs::write(entry, data.to_string_lossy().as_bytes());
    }
}

/// The data folders remembered. A folder with no state left drops out.
pub fn remembered(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home.join("workspaces")) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        let dir = PathBuf::from(&text);
        if text.is_empty() || !dir.join("state").exists() {
            let _ = std::fs::remove_file(e.path());
        } else {
            out.push(dir);
        }
    }
    out
}

/// The folder a data folder serves: its workspace, or rant's home, which is its own.
pub fn folder_of(home: &Path, data: &Path) -> PathBuf {
    if path_key(data) == path_key(home) { data.to_path_buf() } else { data.parent().unwrap_or(data).to_path_buf() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;


    fn instance(name: &str, root: &str) -> Instance {
        Instance {
            name: name.into(),
            node_type: format!("pkg/{name}"),
            argv: vec!["x".into(), "--flag".into()],
            env: BTreeMap::from([("K".into(), "V".into())]),
            cwd: "/w/pkg".into(),
            log: format!("/w/logs/{name}.log"),
            roots: vec![root.into()],
            expect_mesh: true,
            ..Instance::default()
        }
    }

    #[test]
    fn root_keys_name_the_params() {
        let params = BTreeMap::from([("target".to_string(), "red".to_string()), ("a".into(), "1".into())]);
        assert_eq!(Root::group("nav", params).key(), "group nav a=1 target=red");
        assert_eq!(Root::node("talker").key(), "node talker");
        assert_eq!(group_of_key("group nav a=1"), Some("nav"));
        assert_eq!(group_of_key("node nav"), None);
    }

    #[test]
    fn instance_matches_a_name_without_case_or_underscores() {
        let st = State {
            instances: vec![instance("web_explorer", "group explorer/WebExplorer"), instance("Talker", "node Talker"), instance("talker", "node talker")],
            ..State::default()
        };
        assert_eq!(st.instance("WebExplorer").unwrap().name, "web_explorer");
        assert_eq!(st.instance("talker").unwrap().roots[0], "node talker");
        assert_eq!(st.instance("TALKER").unwrap().name, "Talker");
        assert!(st.instance("listener").is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let t = Scratch::new("state-roundtrip");
        let dir = t.0.clone();
        {
            let mut s = Store::open(&dir, true).unwrap();
            s.state().roots.push(Root::node("a"));
            s.state().instances.push(instance("a", "node a"));
            s.save().unwrap();
        }
        let mut s = Store::open(&dir, true).unwrap();
        let st = s.state();
        assert_eq!(st.roots.len(), 1);
        assert_eq!(st.instances[0], instance("a", "node a"));
        assert!(st.instance("a").is_some() && st.root("node a").is_some());
        let text = std::fs::read_to_string(dir.join("state")).unwrap();
        assert!(text.contains("\"type\": \"pkg/a\"") && text.contains("\"restart\": \"never\"") && !text.contains("session"));
    }

    #[test]
    fn prune_drops_dead_instances_and_their_node_roots_only() {
        let mut st = State { roots: vec![Root::node("a"), Root::group("nav", BTreeMap::new())], ..State::default() };
        // pid 0, never alive
        st.instances = vec![instance("a", "node a"), instance("b", "group nav")];
        assert_eq!(prune(&mut st, |_| false).len(), 2);
        assert!(st.instances.is_empty());
        assert_eq!(st.roots, [Root::group("nav", BTreeMap::new())]);
    }

    #[test]
    fn prune_leaves_a_restarting_node_to_the_service_process() {
        let mut st = State::default();
        st.instances.push(Instance { restart: Restart::Always, ..instance("a", "group nav") });
        // gone, and with restart never
        st.instances.push(Instance { pid: 1, ..instance("b", "group nav") });
        // left to the service process of session 2
        st.instances.push(Instance { session: 2, ..instance("c", "group nav") });
        assert_eq!(prune(&mut st, |s| s == 2).len(), 2);
        assert_eq!(st.instances.len(), 1);
        assert_eq!(st.instances[0].name, "c");
        assert_eq!(prune(&mut st, |_| false).len(), 1);
    }

    #[test]
    fn a_held_state_is_not_taken_without_waiting() {
        let t = Scratch::new("state-busy");
        let dir = t.0.clone();
        let first = Store::open(&dir, true).unwrap();
        assert!(first.locked());
        assert!(!Store::open(&dir, false).unwrap().locked());
        drop(first);
        assert!(Store::open(&dir, false).unwrap().locked());
    }

    #[test]
    fn a_damaged_file_is_reported_and_another_format_is_empty() {
        let t = Scratch::new("state-damaged");
        let dir = t.0.clone();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("state"), "{ not json").unwrap();
        assert!(Store::open(&dir, true).is_err());
        std::fs::write(dir.join("state"), "{\"format\": 0, \"roots\": 5}").unwrap();
        assert_eq!(Store::open(&dir, true).unwrap().into_state(), State::default());
    }

    #[test]
    fn remembered_folders_drop_out_once_their_state_is_gone() {
        let t = Scratch::new("state-home");
        let home = t.0.clone();
        let data = home.join("ws/.rant");
        std::fs::create_dir_all(&data).unwrap();
        remember(&home, &data);
        assert!(remembered(&home).is_empty(), "no state file yet");
        remember(&home, &data);
        std::fs::write(data.join("state"), "{}").unwrap();
        assert_eq!(remembered(&home), std::slice::from_ref(&data));
        assert_eq!(folder_of(&home, &data), home.join("ws"));
        assert_eq!(folder_of(&home, &home), home);
    }
}
