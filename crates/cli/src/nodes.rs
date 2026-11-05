//! Running nodes: what start and stop share. Spawning a plan instance into the state, stopping
//! one, and matching the mesh's nodes to the instances rant started.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rant_config::Settings;
use rant_config::plan::{Instance as Planned, Restart};
use rant_process::{Child, Command, Stopped};

use crate::failure::{Result, failed};
use crate::files::path_key;
use crate::mesh::Client;
use crate::services;
use crate::state::{self, Instance, Root, State, Store};

/// How long a node gets to stop on its own before it is killed.
pub const GRACE: Duration = Duration::from_secs(3);

/// A workspace's state, locked and pruned of dead instances, for a command that acts.
pub struct Session {
    store: Store,
}

impl Session {
    pub fn open(home: &Path, data: &Path) -> Result<Session> {
        let mut store = Store::open(data, true)?;
        state::prune(store.state(), |s| services::running_in(home, s));
        Ok(Session { store })
    }

    pub fn state(&mut self) -> &mut State {
        self.store.state()
    }

    pub fn save(&mut self) -> Result<()> {
        self.store.save()
    }

    pub fn into_state(self) -> State {
        self.store.into_state()
    }
}

/// The state as it stands, pruned and written back at once, so a command that only looks
/// holds the lock for a moment.
pub fn snapshot(home: &Path, data: &Path) -> Result<State> {
    let mut s = Session::open(home, data)?;
    s.save()?;
    Ok(s.into_state())
}

/// A job name unique per workspace and node: the same name in another workspace is another
/// job. Global, so every session finds it.
fn job_name(data: &Path, name: &str) -> String {
    format!("Global\\rant-{}-{}", &path_key(data)[..12], name.replace('\\', "_"))
}

/// Starts a record of the workspace whose data folder is data, as a child of this process, its
/// last log moved aside first.
pub fn start(data: &Path, rec: &Instance) -> Result<Child> {
    let log = Path::new(&rec.log);
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if log.exists() {
        let mut previous = log.as_os_str().to_owned();
        previous.push(".1");
        let _ = std::fs::rename(log, previous);
    }
    let mut c = Command { argv: rec.argv.clone(), cwd: rec.cwd.clone().into(), env: rec.env.clone(), base: None };
    if !rec.launch_env.is_empty() {
        c.base = Some(rec.launch_env.clone());
    }
    // the node's mesh name, which holds its prefix already, and unbuffered Python so its log
    // keeps up
    c.env.insert("RANT_NODE_NAME".into(), rec.name.clone());
    c.env.insert("RANT_NODE_NAME_PREFIX".into(), String::new());
    c.env.insert("PYTHONUNBUFFERED".into(), "1".into());
    rant_process::start_child(&c, log, &job_name(data, &rec.name)).map_err(|e| failed(e.to_string()))
}

/// Starts a plan instance under name and gives its record, with no roots. One with restart set,
/// or meant for another session, is left to a service process and has no process until hand_over.
pub fn spawn(home: &Path, ws: &Settings, inst: &Planned, name: &str, session: Option<u32>) -> Result<Instance> {
    // a prefixed name like left/lidar logs to a folder of its prefix
    let log = ws.logs.join(format!("{name}.log"));
    let mut rec = Instance {
        name: name.to_string(),
        node_type: inst.type_ref(),
        argv: inst.argv.clone(),
        env: inst.env.clone(),
        cwd: inst.cwd.to_string_lossy().into_owned(),
        log: log.to_string_lossy().into_owned(),
        group: inst.group.clone().unwrap_or_default(),
        expect_mesh: inst.node.expect_mesh,
        restart: inst.restart,
        ..Instance::default()
    };
    state::remember(home, &ws.data);
    let here = services::session();
    rec.session = session.filter(|&s| s != here && services::running_in(home, s)).unwrap_or(here);
    if rec.session != here {
        rec.launch_env = rant_process::environment(&Command::default());
        return Ok(rec);
    }
    // the service process runs first, so no command drops the record as dead before it starts
    if rec.restart != Restart::Never {
        services::ensure_running(home)?;
        rec.launch_env = rant_process::environment(&Command::default());
    } else {
        rec.track(start(&ws.data, &rec)?.tracking());
    }
    Ok(rec)
}

/// Once the state holding started, in the data folder data, is saved and unlocked: waits until
/// the service process started those spawn left to it, and gives them with their processes.
pub fn hand_over(home: &Path, data: &Path, mut started: Vec<Instance>) -> Result<Vec<Instance>> {
    let waiting = |s: &[Instance]| s.iter().any(|i| i.pid == 0);
    if !waiting(&started) {
        return Ok(started);
    }
    // again, since the state file it needs now exists
    state::remember(home, data);
    for rec in started.iter().filter(|r| r.pid == 0) {
        services::wake(home, rec.session);
    }
    let log = services::log_file(home);
    for i in 0.. {
        if !waiting(&started) {
            break;
        }
        if i == 300 {
            return Err(failed(format!("rant's service process did not start the nodes in 15 s, see {}", log.display())));
        }
        if i > 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        if i % 20 == 19 && !services::running(home) {
            // it ended just before the records came
            services::ensure_running(home)?;
            services::wake(home, services::session());
        }
        let mut store = Store::open(data, true)?;
        for rec in started.iter_mut().filter(|r| r.pid == 0) {
            let now = store.state().instance(&rec.name).ok_or_else(|| failed(format!("rant's service process could not start `{}`, see {}", rec.name, log.display())))?;
            if now.pid != 0 {
                *rec = now.clone();
            }
        }
    }
    Ok(started)
}

/// A node stopped for a while, such as while its package is replaced.
pub struct Paused {
    pub data: PathBuf,
    pub rec: Instance,
}

/// Stops every node any workspace started from inside folder, last started first, and drops it
/// from its state until resume.
pub fn pause_from(home: &Path, folder: &Path) -> Result<Vec<Paused>> {
    let mut out = Vec::new();
    for data in state::remembered(home) {
        let mut s = Session::open(home, &data)?;
        let held: Vec<Instance> = s.state().instances.iter().rev().filter(|i| i.runs_from(folder)).cloned().collect();
        for rec in held {
            halt(home, &rec);
            s.state().forget(&rec.name);
            out.push(Paused { data: data.clone(), rec });
        }
        s.save()?;
    }
    Ok(out)
}

/// Starts paused nodes again in their first order, under their names and roots, leaving one a
/// command started meanwhile be. Returns why each that did not start failed.
pub fn resume(home: &Path, paused: &[Paused]) -> Vec<String> {
    let mut failures = Vec::new();
    let mut folders: Vec<&PathBuf> = Vec::new();
    for p in paused {
        if !folders.contains(&&p.data) {
            folders.push(&p.data);
        }
    }
    for data in folders {
        let mut started = Vec::new();
        let mut store = match Store::open(data, true) {
            Ok(store) => store,
            Err(e) => {
                failures.extend(e.messages());
                continue;
            }
        };
        let st = store.state();
        for p in paused.iter().rev().filter(|p| &p.data == data) {
            if st.instance(&p.rec.name).is_some() {
                continue;
            }
            let mut rec = Instance { pid: 0, start_time: 0, job: String::new(), ..p.rec.clone() };
            // it goes back to its session when the service process there runs
            let there = rec.session != services::session() && services::running_in(home, rec.session);
            if !there {
                rec.session = services::session();
                let begun = if rec.restart != Restart::Never { services::ensure_running(home) } else { start(data, &rec).map(|c| rec.track(c.tracking())) };
                if let Err(e) = begun {
                    failures.push(format!("{}: {}", rec.name, e.messages().join(", ")));
                    continue;
                }
            }
            let own = format!("node {}", rec.name);
            if rec.roots.contains(&own) && st.root(&own).is_none() {
                st.roots.push(Root::node(&rec.name));
            }
            st.instances.push(rec.clone());
            started.push(rec);
        }
        if let Err(e) = store.save().and_then(|_| hand_over(home, data, started)) {
            failures.extend(e.messages());
        }
    }
    failures
}

/// An instance that exited, and how long after the watch began it was seen gone.
pub struct Exit {
    pub name: String,
    pub after: Duration,
}

/// The instances of started that exited within span, sooner once all of them have.
pub fn exited_within(started: &[Instance], span: Duration) -> Vec<Exit> {
    let begin = Instant::now();
    let mut out: Vec<Exit> = Vec::new();
    loop {
        for i in started {
            if !out.iter().any(|e| e.name == i.name) && !rant_process::alive(&i.tracking()) {
                out.push(Exit { name: i.name.clone(), after: begin.elapsed() });
            }
        }
        let left = span.saturating_sub(begin.elapsed());
        if out.len() == started.len() || left.is_zero() {
            return out;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Stops an instance and everything it started: gracefully, then by force after GRACE. One in
/// another session is stopped by the service process there.
pub fn halt(home: &Path, inst: &Instance) -> Stopped {
    // Ctrl-Break and window messages reach no other session
    if inst.session != services::session()
        && let Some(how) = services::stop_through(home, inst.session, &inst.tracking())
    {
        return how;
    }
    rant_process::stop(&inst.tracking(), GRACE)
}

/// A running instance is the plan's node: same type, argv, env and working directory.
pub fn same_spec(running: &Instance, planned: &Planned) -> bool {
    running.node_type == planned.type_ref() && running.argv == planned.argv && running.env == planned.env && Path::new(&running.cwd) == planned.cwd
}

/// What releasing a root did to one of its nodes.
pub struct Released {
    pub name: String,
    /// The roots still needing it, empty when stopped.
    pub kept_for: Vec<String>,
    pub how: Stopped,
}

/// Drops a group's root. Each of its nodes no other group needs stops, last started first,
/// even one also started on its own, and a node another group still needs keeps running.
pub fn release_root(home: &Path, st: &mut State, key: &str) -> Vec<Released> {
    st.roots.retain(|r| r.key() != key);
    let mut out = Vec::new();
    for i in (0..st.instances.len()).rev() {
        let inst = &mut st.instances[i];
        let Some(at) = inst.roots.iter().position(|k| k == key) else { continue };
        inst.roots.remove(at);
        if inst.held_by_group() {
            out.push(Released { name: inst.name.clone(), kept_for: inst.roots.clone(), how: Stopped::AlreadyGone });
            continue;
        }
        let r = Released { name: inst.name.clone(), kept_for: Vec::new(), how: halt(home, inst) };
        st.forget(&r.name);
        out.push(r);
    }
    out
}

/// The peers that are instances rant started, by peer id, with the instance name. The pid a peer
/// reports is in the instance's group or job, so a node behind a wrapper still matches.
pub fn managed_peers(mesh: &Client, st: &State) -> BTreeMap<u32, String> {
    if st.instances.is_empty() {
        return BTreeMap::new();
    }
    let ids: Vec<u32> = mesh.peers().iter().map(|p| p.id).collect();
    let mut out = BTreeMap::new();
    for (peer, pid) in mesh.pids(&ids, Duration::from_millis(500)) {
        if let Some(inst) = st.instances.iter().find(|i| rant_process::owns(&i.tracking(), pid)) {
            out.insert(peer, inst.name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_named_per_workspace_and_node() {
        let a = job_name(Path::new("/w/a/.rant"), "left\\lidar");
        assert!(a.starts_with("Global\\rant-") && a.ends_with("-left_lidar"));
        assert_ne!(a, job_name(Path::new("/w/b/.rant"), "left\\lidar"));
    }
}
