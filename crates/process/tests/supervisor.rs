//! Detached starts, tracking and stopping, against cmake, a program on every OS that sleeps
//! and exits with a code. Its own main, since stopping on Windows runs this exe as a helper.

use std::path::PathBuf;
use std::time::Duration;

use rant_process::{Command, Stopped, Tracking, alive, helper, owns, start_child, start_detached, stop};

fn main() {
    if let Some(code) = helper(std::env::args_os()) {
        std::process::exit(code);
    }
    let tests: [(&str, fn()); 4] = [
        ("a_detached_start_is_alive_until_stopped", a_detached_start_is_alive_until_stopped),
        ("a_process_that_exits_is_not_alive", a_process_that_exits_is_not_alive),
        ("a_child_tells_its_exit_code", a_child_tells_its_exit_code),
        ("an_untrackable_id_is_never_signalled", an_untrackable_id_is_never_signalled),
    ];
    let failed = tests.iter().filter(|(name, test)| {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        !ok
    });
    let failed = failed.count();
    println!("\ntest result: {}. {} passed; {failed} failed", if failed == 0 { "ok" } else { "FAILED" }, tests.len() - failed);
    std::process::exit(if failed == 0 { 0 } else { 1 });
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rant-process-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cmake(args: &[&str], dir: &PathBuf) -> Command {
    Command::new(std::iter::once("cmake").chain(args.iter().copied()), dir)
}

fn job(name: &str) -> String {
    format!("Local\\rant-process-test-{name}-{}", std::process::id())
}

fn eventually(mut done: impl FnMut() -> bool) -> bool {
    (0..250).any(|_| {
        std::thread::sleep(Duration::from_millis(20));
        done()
    })
}

fn a_detached_start_is_alive_until_stopped() {
    let dir = scratch("detached");
    let t = start_detached(&cmake(&["-E", "sleep", "30"], &dir), &dir.join("out.log"), &job("detached")).unwrap();
    assert!(t.pid != 0);
    assert!(alive(&t));
    assert!(owns(&t, t.pid));
    assert_eq!(stop(&t, Duration::from_secs(3)), Stopped::Gracefully);
    assert!(!alive(&t));
    assert_eq!(stop(&t, Duration::from_secs(1)), Stopped::AlreadyGone);
}

fn a_process_that_exits_is_not_alive() {
    let dir = scratch("exits");
    let t = start_detached(&cmake(&["-E", "true"], &dir), &dir.join("out.log"), &job("exits")).unwrap();
    assert!(eventually(|| !alive(&t)));
}

fn a_child_tells_its_exit_code() {
    let dir = scratch("child");
    let mut ok = start_child(&cmake(&["-E", "true"], &dir), &dir.join("ok.log"), &job("ok")).unwrap();
    let mut bad = start_child(&cmake(&["-E", "false"], &dir), &dir.join("bad.log"), &job("bad")).unwrap();
    let mut slow = start_child(&cmake(&["-E", "sleep", "30"], &dir), &dir.join("slow.log"), &job("slow")).unwrap();
    assert!(eventually(|| ok.ended().is_some() && bad.ended().is_some()));
    assert_eq!((ok.ended(), bad.ended()), (Some(0), Some(1)));
    assert_eq!(slow.ended(), None);
    stop(slow.tracking(), Duration::from_secs(3));
    assert!(eventually(|| slow.ended().is_some()));
}

fn an_untrackable_id_is_never_signalled() {
    for pid in [0, 1] {
        let t = Tracking { pid, ..Tracking::default() };
        assert!(!alive(&t));
        assert_eq!(stop(&t, Duration::from_millis(10)), Stopped::AlreadyGone);
    }
}
