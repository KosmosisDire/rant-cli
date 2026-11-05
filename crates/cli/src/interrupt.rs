//! Ctrl-C caught, so a command that runs until it comes ends cleanly.

use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Catches Ctrl-C and Ctrl-Break from here on.
#[cfg(windows)]
pub fn catch() {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};
    unsafe extern "system" fn on_ctrl(kind: u32) -> i32 {
        let ours = kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT;
        if ours {
            INTERRUPTED.store(true, Ordering::Relaxed);
        }
        ours as i32
    }
    // SAFETY: the handler only sets an atomic.
    unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
}

/// Catches SIGINT and SIGTERM from here on.
#[cfg(unix)]
pub fn catch() {
    extern "C" fn on_signal(_: libc::c_int) {
        INTERRUPTED.store(true, Ordering::Relaxed);
    }
    // SAFETY: the handler only sets an atomic, which is async signal safe.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}
