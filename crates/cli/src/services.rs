//! The service process, `rant __services`, one per rant home and session. It starts the nodes
//! with restart set as its children, starts them again as their restart says, then ends.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rant_config::plan::Restart;
use rant_process::{Child, Command, Doorbell, Stopped, Tracking};
use serde::{Deserialize, Serialize};

use crate::failure::{Result, failed};
use crate::files::path_key;
use crate::nodes;
use crate::state::{self, Store};

/// Where it writes what it starts and why.
pub fn log_file(home: &Path) -> PathBuf {
    home.join("logs").join("services.log")
}

/// Tells this home's service process from another home's, as a test's.
fn home_key(home: &Path) -> String {
    path_key(home)[..12].to_string()
}

/// Each session has its own, since a process gets Ctrl-Break and window messages only from
/// its own session.
pub fn session() -> u32 {
    rant_process::session()
}

fn lock_file(home: &Path, s: u32) -> PathBuf {
    if cfg!(windows) { home.join(format!("services-{s}.lock")) } else { home.join("services.lock") }
}

/// Global on Windows, so a command in another session reaches it.
fn doorbell(home: &Path, s: u32) -> String {
    if cfg!(windows) { format!("Global\\rant-services-{}-{s}", home_key(home)) } else { home.join("services.wake").to_string_lossy().into_owned() }
}

/// Asks the one of a session to look for nodes to start now rather than at its next round.
pub fn wake(home: &Path, s: u32) {
    rant_process::ring(&doorbell(home, s));
}

/// The lock the running one holds, taken without waiting.
fn try_lock(home: &Path, s: u32) -> Option<File> {
    let _ = std::fs::create_dir_all(home);
    let f = File::options().read(true).write(true).create(true).truncate(false).open(lock_file(home, s)).ok()?;
    f.try_lock().is_ok().then_some(f)
}

pub fn running_in(home: &Path, s: u32) -> bool {
    try_lock(home, s).is_none()
}

pub fn running(home: &Path) -> bool {
    running_in(home, session())
}

/// A stop request is a file named after the session and the node, answered by a file saying
/// how it stopped.
fn stops(home: &Path) -> PathBuf {
    home.join("stops")
}

/// Asks the one of another session to stop a node there, and waits until it says how. None
/// when none runs there or it does not answer, and then the caller stops the node itself.
pub fn stop_through(home: &Path, s: u32, t: &Tracking) -> Option<Stopped> {
    if !running_in(home, s) {
        return None;
    }
    let _ = std::fs::create_dir_all(stops(home));
    let base = format!("{s}-{}-{}", t.pid, t.start_time);
    let (ask, done) = (stops(home).join(format!("{base}.ask")), stops(home).join(format!("{base}.done")));
    let _ = std::fs::write(&ask, format!("{} {} {}\n", t.pid, t.start_time, t.job));
    wake(home, s);
    for _ in 0..150 {
        if let Ok(how) = std::fs::read_to_string(&done) {
            let _ = std::fs::remove_file(&done);
            return Some(match how.trim() {
                "gracefully" => Stopped::Gracefully,
                "killed" => Stopped::Killed,
                _ => Stopped::AlreadyGone,
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&ask);
    None
}

/// Starts the one of this session unless it runs, and waits until it does.
pub fn ensure_running(home: &Path) -> Result<()> {
    if running(home) {
        return Ok(());
    }
    let _ = std::fs::create_dir_all(home.join("logs"));
    let job = format!("Local\\rant-services-{}-{}", home_key(home), stamp_nanos());
    let me = std::env::current_exe().map_err(|e| failed(format!("cannot find rant itself: {e}")))?;
    rant_process::start_detached(&Command::new([me.to_string_lossy().into_owned(), "__services".into()], home), &log_file(home), &job)
        .map_err(|e| failed(e.to_string()))?;
    if rant_process::wait_until(Duration::from_secs(10), || running(home)) {
        return Ok(());
    }
    Err(failed(format!("rant's service process did not start, see {}", log_file(home).display())))
}

fn stamp_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// A workspace rant enable enabled, or the rant home outside one, whose services the service
/// process starts at every boot or login with a bare rant run in its folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Enabled {
    pub folder: PathBuf,
    #[serde(default = "boot")]
    pub start: When,
    /// --domain as given, which every node then takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<u16>,
    /// The placement variables rant enable saw.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum When {
    Boot,
    Login,
}

fn boot() -> When {
    When::Boot
}

fn list_file(home: &Path) -> PathBuf {
    home.join("services.json")
}

/// What is enabled, in the order it was.
pub fn enabled(home: &Path) -> Result<Vec<Enabled>> {
    let Ok(bytes) = std::fs::read(list_file(home)) else { return Ok(Vec::new()) };
    let damaged = || failed(format!("{} is damaged, delete it and enable again", list_file(home).display()));
    let doc: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| damaged())?;
    let all = doc.get("enabled").and_then(serde_json::Value::as_array).ok_or_else(damaged)?;
    // an entry with a kind is an older rant's for one group, which nothing starts now
    let current = all.iter().filter(|j| j.get("kind").is_none());
    current.map(|j| serde_json::from_value(j.clone()).map_err(|_| damaged())).collect()
}

/// One line of the log, after the local time.
fn say(home: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = File::options().append(true).create(true).open(log_file(home)) {
        let _ = writeln!(f, "{} {line}", local_time());
    }
}

#[cfg(windows)]
fn local_time() -> String {
    // SAFETY: GetLocalTime only writes the struct.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(unix)]
fn local_time() -> String {
    // SAFETY: localtime_r writes only the struct it is given.
    let t = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut t = std::mem::zeroed::<libc::tm>();
        libc::localtime_r(&now, &mut t);
        t
    };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.tm_year + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)
}

fn same(a: &Tracking, b: &Tracking) -> bool {
    a.pid == b.pid && a.start_time == b.start_time
}

/// How long a node that keeps failing soon after it starts waits before the next start.
fn pause(quick: u32) -> Duration {
    Duration::from_secs(60.min(1 << quick.saturating_sub(1).min(6)))
}

/// A node is quick to fail when it ends this soon after starting.
const SETTLED: Duration = Duration::from_secs(10);

/// Runs a bare rant run, detached, in each folder enabled to start then. They hand their nodes
/// with restart set to the service process, so none waits for them.
fn start_enabled(home: &Path, when: When) {
    let list = enabled(home).unwrap_or_else(|e| {
        e.messages().iter().for_each(|m| say(home, m));
        Vec::new()
    });
    let Ok(me) = std::env::current_exe() else { return };
    for e in list.iter().filter(|e| e.start == when) {
        let folder = e.folder.display().to_string();
        if !e.folder.is_dir() {
            say(home, &format!("cannot start the services of {folder}, it is gone"));
            continue;
        }
        let mut argv = vec![me.to_string_lossy().into_owned(), "run".into(), "--no-build".into(), "-y".into()];
        if let Some(d) = e.domain {
            argv.extend(["--domain".into(), d.to_string()]);
        }
        let job = format!("Local\\rant-services-{}-start-{}", home_key(home), stamp_nanos());
        let word = if when == When::Boot { "boot" } else { "login" };
        say(home, &format!("at {word}, starting the services of {folder}"));
        let c = Command { argv, cwd: e.folder.clone(), env: e.env.clone(), base: None };
        if let Err(f) = rant_process::start_detached(&c, &log_file(home), &job) {
            say(home, &format!("cannot start the services of {folder}: {f}"));
        }
    }
}

/// A node this process watches: one it started, or one a former service process started,
/// whose exit code it cannot learn.
struct Watched {
    data: PathBuf,
    name: String,
    tracking: Tracking,
    child: Option<Child>,
    since: Instant,
    /// Quick failures in a row before this start.
    quick: u32,
}

/// A node that ended and waits for its workspace's lock, or for its time to start again.
struct Ended {
    data: PathBuf,
    name: String,
    tracking: Tracking,
    /// None when not known.
    code: Option<i32>,
    quick: u32,
    again: Option<Instant>,
    /// When its record was last read while it waits.
    checked: Option<Instant>,
}

struct Host {
    home: PathBuf,
    watched: Vec<Watched>,
    ended: Vec<Ended>,
    looked: Option<Instant>,
    look_again: bool,
    /// Nodes of this session alive at the last look.
    live_here: usize,
    idle_since: Instant,
}

impl Host {
    fn new(home: &Path) -> Host {
        Host { home: home.to_path_buf(), watched: Vec::new(), ended: Vec::new(), looked: None, look_again: true, live_here: 0, idle_since: Instant::now() }
    }

    /// One round: notes what ended, starts what is due, and looks at every workspace when
    /// asked to or now and then. False once it has had nothing to do for a while.
    fn round(&mut self, woken: bool) -> bool {
        self.answer_stops();
        self.note_ended();
        self.decide();
        if woken || self.look_again || self.looked.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.look();
            self.looked = Some(Instant::now());
        }
        // session 0, where the boot task runs, has no desktop or terminal, so only this
        // process can stop its nodes gently
        let keep = cfg!(windows) && session() == 0 && self.live_here > 0;
        if keep || !self.watched.is_empty() || !self.ended.is_empty() || self.look_again {
            self.idle_since = Instant::now();
        }
        self.idle_since.elapsed() < Duration::from_secs(10)
    }

    fn note_ended(&mut self) {
        let mut still = Vec::new();
        for mut w in std::mem::take(&mut self.watched) {
            let code = match &mut w.child {
                Some(child) => child.ended().map(Some),
                None => (!rant_process::alive(&w.tracking)).then_some(None),
            };
            match code {
                None => still.push(w),
                Some(code) => {
                    let quick = if w.since.elapsed() < SETTLED { w.quick + 1 } else { 0 };
                    self.ended.push(Ended { data: w.data, name: w.name, tracking: w.tracking, code, quick, again: None, checked: None });
                }
            }
        }
        self.watched = still;
    }

    /// A node a command stopped or started again has another record or none, so it is left be.
    fn decide(&mut self) {
        let mut waiting = Vec::new();
        for mut e in std::mem::take(&mut self.ended) {
            if self.decide_one(&mut e) {
                waiting.push(e);
            }
        }
        self.ended.extend(waiting);
    }

    /// True while it still waits.
    fn decide_one(&mut self, e: &mut Ended) -> bool {
        // while it waits its record is read now and then, so a stop takes effect soon
        let due = e.again.is_none_or(|t| Instant::now() >= t);
        if !due && e.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return true;
        }
        let Ok(mut store) = Store::open(&e.data, false) else { return true };
        if !store.locked() {
            return true;
        }
        let Some(rec) = store.state().instance(&e.name).filter(|r| same(&r.tracking(), &e.tracking)).cloned() else { return false };
        if !due {
            e.checked = Some(Instant::now());
            return true;
        }
        let code = e.code.map_or_else(|| "an unknown code".to_string(), |c| format!("code {c}"));
        let home = self.home.clone();
        if e.again.is_none() {
            let again = rec.restart == Restart::Always || (rec.restart == Restart::OnFailure && e.code != Some(0));
            if !again {
                say(&home, &format!("{} exited with {code}, and its restart is {}", e.name, rec.restart.as_str()));
                store.state().forget(&e.name);
                let _ = store.save();
                return false;
            }
            let wait = pause(e.quick);
            say(&home, &format!("{} exited with {code}, starting it again in {} s", e.name, wait.as_secs()));
            e.again = Some(Instant::now() + wait);
            return true;
        }
        match nodes::start(&e.data, &rec) {
            Ok(child) => {
                let r = store.state().instance_mut(&e.name).expect("found above");
                r.track(child.tracking());
                r.restarts += 1;
                let restarts = r.restarts;
                let _ = store.save();
                say(&home, &format!("started {} again, restart {restarts}", e.name));
                let tracking = child.tracking().clone();
                self.watched.push(Watched { data: e.data.clone(), name: e.name.clone(), tracking, child: Some(child), since: Instant::now(), quick: e.quick });
                false
            }
            Err(f) => {
                say(&home, &format!("cannot start {} again: {}", e.name, f.messages().join(", ")));
                e.again = Some(Instant::now() + pause(e.quick + 1));
                e.quick += 1;
                true
            }
        }
    }

    fn watching(&self, data: &Path, rec: &state::Instance) -> bool {
        let mine = |d: &Path, n: &str| d == data && n == rec.name;
        self.watched.iter().any(|w| mine(&w.data, &w.name) && same(&w.tracking, &rec.tracking())) || self.ended.iter().any(|e| mine(&e.data, &e.name))
    }

    /// Starts the nodes commands left to it, and takes on those a former service process started.
    /// A workspace whose state is busy waits for the next round.
    fn look(&mut self) {
        self.look_again = false;
        self.live_here = 0;
        let home = self.home.clone();
        for data in state::remembered(&home) {
            let Ok(mut store) = Store::open(&data, false) else { continue };
            if !store.locked() {
                self.look_again = true;
                continue;
            }
            let mut changed = false;
            let mut i = 0;
            while i < store.state().instances.len() {
                let rec = store.state().instances[i].clone();
                i += 1;
                if rec.session != session() {
                    continue;
                }
                let alive = rec.pid != 0 && rant_process::alive(&rec.tracking());
                self.live_here += alive as usize;
                if self.watching(&data, &rec) {
                    continue;
                }
                if rec.pid == 0 {
                    changed = true;
                    match nodes::start(&data, &rec) {
                        Ok(child) => {
                            store.state().instances[i - 1].track(child.tracking());
                            let folder = state::folder_of(&home, &data);
                            say(&home, &format!("started {} in {}, restart {}", rec.name, folder.display(), rec.restart.as_str()));
                            let tracking = child.tracking().clone();
                            self.watched.push(Watched { data: data.clone(), name: rec.name, tracking, child: Some(child), since: Instant::now(), quick: 0 });
                        }
                        Err(f) => {
                            say(&home, &format!("cannot start {}: {}", rec.name, f.messages().join(", ")));
                            store.state().forget(&rec.name);
                            // the next record took its place
                            i -= 1;
                        }
                    }
                } else if rec.restart == Restart::Never {
                    continue;
                } else if alive {
                    say(&home, &format!("watching {}, which an earlier service process started", rec.name));
                    self.watched.push(Watched { data: data.clone(), name: rec.name.clone(), tracking: rec.tracking(), child: None, since: Instant::now(), quick: 0 });
                } else {
                    self.ended.push(Ended { data: data.clone(), name: rec.name.clone(), tracking: rec.tracking(), code: None, quick: 0, again: None, checked: None });
                }
            }
            if changed {
                let _ = store.save();
            }
        }
    }

    /// Stops the nodes of this session commands of another session asked it to stop.
    fn answer_stops(&self) {
        let mine = format!("{}-", session());
        let Ok(entries) = std::fs::read_dir(stops(&self.home)) else { return };
        for f in entries.flatten() {
            let path = f.path();
            let name = f.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&mine) || path.extension().is_none_or(|x| x != "ask") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let _ = std::fs::remove_file(&path);
            let mut words = text.trim().splitn(3, ' ');
            let mut number = || words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
            let (pid, start_time) = (number(), number());
            let t = Tracking { pid, start_time, job: words.next().unwrap_or("").to_string() };
            let word = match rant_process::stop(&t, nodes::GRACE) {
                Stopped::Gracefully => "gracefully",
                Stopped::AlreadyGone => "gone",
                _ => "killed",
            };
            let _ = std::fs::write(path.with_extension("done"), format!("{word}\n"));
        }
    }
}

/// The service process itself. start: first start what is enabled to start then, and a
/// service process to watch it, then end.
pub fn serve(home: &Path, start: Option<When>) -> i32 {
    let _ = std::fs::create_dir_all(home.join("logs"));
    // an entry only starts what is enabled and a service process to watch it, then ends, so
    // the console window older Windows gives it closes and no service manager holds the nodes
    if let Some(when) = start {
        start_enabled(home, when);
        if let Err(e) = ensure_running(home) {
            e.messages().iter().for_each(|m| say(home, m));
            return 1;
        }
        return 0;
    }
    // a command probing whether one runs holds the lock for a moment, so a few tries
    let mut lock = None;
    for _ in 0..20 {
        lock = try_lock(home, session());
        if lock.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if lock.is_none() {
        // another one runs
        return 0;
    }
    let bell = Doorbell::listen(&doorbell(home, session()));
    let wait = || bell.as_ref().map_or_else(|| { std::thread::sleep(Duration::from_millis(200)); false }, |b| b.wait(Duration::from_millis(200)));
    say(home, &format!("service process {} started", std::process::id()));
    let mut host = Host::new(home);
    let mut woken = true;
    while host.round(woken) {
        woken = wait();
    }
    say(home, &format!("service process {} stopped, nothing to watch", std::process::id()));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_that_keeps_failing_waits_longer_each_time_up_to_a_minute() {
        let waits: Vec<u64> = (0..9).map(|q| pause(q).as_secs()).collect();
        assert_eq!(waits, [1, 1, 2, 4, 8, 16, 32, 60, 60]);
    }

    #[test]
    fn the_enabled_list_reads_and_skips_old_entries() {
        let home = std::env::temp_dir().join(format!("rant-unit-enabled-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        assert!(enabled(&home).unwrap().is_empty());
        std::fs::write(list_file(&home), r#"{"enabled": [{"folder": "ws", "start": "login", "domain": 7, "env": {"RANT_PREFIX": "cell"}}]}"#).unwrap();
        let env = BTreeMap::from([("RANT_PREFIX".into(), "cell".into())]);
        assert_eq!(enabled(&home).unwrap(), [Enabled { folder: "ws".into(), start: When::Login, domain: Some(7), env }]);
        std::fs::write(list_file(&home), r#"{"enabled": [{"kind": "group", "folder": "x"}, {"folder": "y"}]}"#).unwrap();
        assert_eq!(enabled(&home).unwrap(), [Enabled { folder: "y".into(), start: When::Boot, domain: None, env: BTreeMap::new() }]);
        std::fs::write(list_file(&home), r#"{"enabled": [{"folder": "y", "start": "noon"}]}"#).unwrap();
        assert!(enabled(&home).is_err());
        std::fs::remove_dir_all(&home).unwrap();
    }
}
