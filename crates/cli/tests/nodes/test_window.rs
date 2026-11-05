//! A Windows GUI program for the end to end tests, built as the bridge is: no console, and a
//! hidden window whose closing stops it, which is how rant stop asks one. test_window [--life S]
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, MSG, RegisterClassW, SetTimer, WM_TIMER, WNDCLASSW,
    };

    let mut args = std::env::args().skip(1);
    let mut life = 60.0;
    while let Some(word) = args.next() {
        if word == "--life" {
            life = args.next().and_then(|s| s.parse().ok()).unwrap_or(life);
        }
    }
    let class = wide("rant_test_window");
    let title = wide("test_window");
    // SAFETY: plain Win32 calls on strings that outlive them
    unsafe {
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW { lpfnWndProc: Some(on_message), hInstance: instance, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let window = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
        if window.is_null() {
            std::process::exit(1);
        }
        say("ready test_window");
        // a failed test must not leave it running long
        SetTimer(window, 1, (life * 1000.0) as u32, None);
        let mut m: MSG = std::mem::zeroed();
        while GetMessageW(&mut m, null_mut(), 0, 0) > 0 {
            if m.message == WM_TIMER {
                std::process::exit(1);
            }
            DispatchMessageW(&m);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn on_message(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, PostQuitMessage, WM_CLOSE};
    if message == WM_CLOSE {
        say("stopped");
        // SAFETY: called on the thread that owns the message loop
        unsafe { PostQuitMessage(0) };
        return 0;
    }
    // SAFETY: the arguments are the ones Windows passed in
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

#[cfg(windows)]
fn say(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}
