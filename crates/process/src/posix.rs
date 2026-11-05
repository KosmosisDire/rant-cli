//! The POSIX side: programs run through std's fork and exec with every fd but stdio closed at
//! exec, and nodes tracked as process groups.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::{Command, Result, Stopped, Tracking, environment, error, require_program, wait_until};

pub(crate) fn runnable(p: &Path) -> Option<PathBuf> {
    let executable = std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    executable.then(|| p.to_path_buf())
}

pub(crate) fn inherited_environment() -> Vec<String> {
    std::env::vars_os().map(|(k, v)| format!("{}={}", k.to_string_lossy(), v.to_string_lossy())).collect()
}

/// The std command for c, argv[0] kept as given. A lock, socket or pipe of rant never leaks
/// into what it starts, and a detached start leads a session of its own.
fn command(c: &Command, detached: bool) -> Result<std::process::Command> {
    let program = require_program(c)?;
    if !c.cwd.is_dir() {
        return Err(error(format!("cannot enter {}: no such folder", c.cwd.display())));
    }
    let mut cmd = std::process::Command::new(program);
    cmd.arg0(&c.argv[0]).args(&c.argv[1..]).current_dir(&c.cwd).env_clear();
    for entry in environment(c) {
        if let Some((k, v)) = entry.split_once('=') {
            cmd.env(k, v);
        }
    }
    // SAFETY: only async signal safe calls run between fork and exec.
    unsafe {
        cmd.pre_exec(move || {
            if detached {
                libc::setsid();
            }
            close_others_at_exec();
            Ok(())
        });
    }
    Ok(cmd)
}

/// Marks every fd above stdio close on exec. std's own pipe that reports a failed exec is
/// close on exec already, so it still works.
fn close_others_at_exec() {
    // close_range, the same syscall number on every Linux architecture, on kernels from 5.11
    #[cfg(target_os = "linux")]
    {
        const CLOSE_RANGE: libc::c_long = 436;
        const CLOEXEC: libc::c_uint = 1 << 2;
        // SAFETY: a plain syscall on fd numbers.
        if unsafe { libc::syscall(CLOSE_RANGE, 3 as libc::c_uint, libc::c_uint::MAX, CLOEXEC) } == 0 {
            return;
        }
    }
    // SAFETY: sysconf and fcntl are async signal safe and touch no memory.
    unsafe {
        let max = libc::sysconf(libc::_SC_OPEN_MAX);
        let max = if (0..=65536).contains(&max) { max as libc::c_int } else { 65536 };
        for fd in 3..max {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1)
}

pub(crate) fn run(c: &Command) -> Result<i32> {
    let mut cmd = command(c, false)?;
    cmd.stdout(Stdio::from(std::io::stderr()));
    let status = cmd.status().map_err(|e| error(format!("cannot run {}: {e}", c.argv[0])))?;
    Ok(exit_code(status))
}

pub(crate) fn output(c: &Command) -> Option<String> {
    let mut cmd = command(c, false).ok()?;
    let done = cmd.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    done.status.success().then(|| String::from_utf8_lossy(&done.stdout).into_owned())
}

/// A detached process this one started, which it may ask for its exit code.
pub struct Child {
    tracking: Tracking,
    code: Option<i32>,
}

impl Child {
    pub fn tracking(&self) -> &Tracking {
        &self.tracking
    }

    /// Its exit code once all of it ended, 128 plus the signal for a killed one, None while
    /// any process of it runs.
    pub fn ended(&mut self) -> Option<i32> {
        if self.code.is_none() && !alive(&self.tracking) {
            // a code never seen, as for a child another process reaped, is no success
            self.code = Some(reaped().remove(&(self.tracking.pid as i32)).unwrap_or(1));
        }
        self.code
    }
}

pub fn start_child(c: &Command, log: &Path, _job: &str) -> Result<Child> {
    let file = OpenOptions::new().append(true).create(true).mode(0o644).open(log).map_err(|e| error(format!("cannot open {}: {e}", log.display())))?;
    let out = file.try_clone().map_err(|e| error(format!("cannot open {}: {e}", log.display())))?;
    let mut cmd = command(c, true)?;
    cmd.stdin(Stdio::null()).stdout(out).stderr(file);
    let pid = cmd.spawn().map_err(|e| error(format!("cannot run {}: {e}", c.argv[0])))?.id() as u64;
    let start_time = start_time(pid as i32).unwrap_or(0);
    Ok(Child { tracking: Tracking { pid, start_time, job: String::new() }, code: None })
}

/// The exit codes of rant's own children that reap collected, until a Child asks for its own.
fn reaped() -> MutexGuard<'static, BTreeMap<i32, i32>> {
    static REAPED: Mutex<BTreeMap<i32, i32>> = Mutex::new(BTreeMap::new());
    REAPED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Collects rant's own exited child, so a node started in this run never lingers as a zombie
/// that still counts as a group member. For another process's child nothing happens.
fn reap(pid: i32) {
    let mut status = 0;
    // SAFETY: waitpid only writes the status it is given.
    if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == pid {
        reaped().insert(pid, exit_code(ExitStatus::from_raw(status)));
    }
}

/// When a process started, in a unit the OS keeps for its life: clock ticks since boot on
/// Linux, microseconds since the epoch on macOS. None when it is gone.
#[cfg(not(target_os = "macos"))]
fn start_time(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // the name may hold spaces and parentheses, and field 22 counts from after it
    let rest = &stat[stat.rfind(')')? + 2..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
fn start_time(pid: i32) -> Option<u64> {
    // SAFETY: proc_pidinfo writes at most the size given into info.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// kill(-0) would signal rant's own group and kill(-1) every process it may, so an id below
/// 2, which no started node has, is never used.
fn trackable(pid: u64) -> bool {
    pid > 1 && pid <= i32::MAX as u64
}

fn errno_is(code: i32) -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(code)
}

pub fn alive(t: &Tracking) -> bool {
    if !trackable(t.pid) {
        return false;
    }
    let group = t.pid as i32;
    reap(group);
    // SAFETY: signal 0 only checks.
    if unsafe { libc::kill(-group, 0) } != 0 && !errno_is(libc::EPERM) {
        return false;
    }
    // the group id stays taken while any member lives, and only a leader with another start
    // time means the id went to a new group
    start_time(group).is_none_or(|st| t.start_time == 0 || st == t.start_time)
}

pub fn owns(t: &Tracking, pid: u64) -> bool {
    // SAFETY: getpgid only reads.
    trackable(pid) && alive(t) && unsafe { libc::getpgid(pid as i32) } == t.pid as i32
}

pub fn pid_alive(pid: u64) -> bool {
    // SAFETY: signal 0 only checks.
    trackable(pid) && (unsafe { libc::kill(pid as i32, 0) } == 0 || errno_is(libc::EPERM))
}

/// Asks the whole group to stop with SIGTERM, waits up to grace, then kills what is left.
pub fn stop(t: &Tracking, grace: Duration) -> Stopped {
    if !alive(t) {
        return Stopped::AlreadyGone;
    }
    let group = -(t.pid as i32);
    // SAFETY: the group is a tracked one above 1.
    unsafe { libc::kill(group, libc::SIGTERM) };
    if wait_until(grace, || !alive(t)) {
        return Stopped::Gracefully;
    }
    unsafe { libc::kill(group, libc::SIGKILL) };
    wait_until(Duration::from_secs(2), || !alive(t));
    Stopped::Killed
}

/// Stops a process rant did not start, by its id alone: SIGTERM, then SIGKILL after grace.
pub fn stop_pid(pid: u64, grace: Duration) -> Result<Stopped> {
    if !trackable(pid) || pid == std::process::id() as u64 {
        return Ok(Stopped::AlreadyGone);
    }
    let p = pid as i32;
    // SAFETY: p is one process above 1, never this one.
    if unsafe { libc::kill(p, libc::SIGTERM) } != 0 {
        if errno_is(libc::EPERM) {
            return Err(error(format!("process {pid} belongs to another user")));
        }
        return Ok(Stopped::AlreadyGone);
    }
    if wait_until(grace, || unsafe { libc::kill(p, 0) } != 0) {
        return Ok(Stopped::Gracefully);
    }
    unsafe { libc::kill(p, libc::SIGKILL) };
    Ok(Stopped::Killed)
}

/// Only Windows runs rant as a helper to stop a node.
pub fn helper(_args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<i32> {
    None
}
