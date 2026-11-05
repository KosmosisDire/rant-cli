//! A block of lines redrawn in place on stderr. It fits the terminal, with a line saying how
//! much did not, so the cursor always lands back on its first line.

use std::io::Write;

pub struct LiveView {
    drawn: usize,
}

impl LiveView {
    pub fn new() -> LiveView {
        put("\x1b[?25l");
        LiveView { drawn: 0 }
    }

    pub fn draw(&mut self, lines: &[String]) {
        let (rows, cols) = terminal_size();
        let cols = if cols > 1 { cols - 1 } else { 200 };
        let room = if rows > 2 { rows - 1 } else { lines.len() };
        let mut frame = self.back();
        let shown = if lines.len() <= room { lines.len() } else { room - 1 };
        for l in &lines[..shown] {
            frame += &(clip(l, cols) + "\n");
        }
        if shown < lines.len() {
            frame += &(clip(&format!("... {} more lines", lines.len() - shown), cols) + "\n");
        }
        self.drawn = shown + usize::from(shown < lines.len());
        put(&frame);
    }

    /// Takes the last frame off the screen, so what replaces it prints in its place.
    pub fn erase(mut self) {
        put(&self.back());
        self.drawn = 0;
    }

    /// Back to the first line of the last frame, and the screen below it cleared.
    fn back(&self) -> String {
        let up = if self.drawn > 0 { format!("\x1b[{}F", self.drawn) } else { String::new() };
        up + "\x1b[J"
    }
}

impl Drop for LiveView {
    fn drop(&mut self) {
        put("\x1b[?25h");
    }
}

fn put(s: &str) {
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(s.as_bytes());
    let _ = err.flush();
}

/// text cut to cols visible characters, color codes kept whole and closed.
pub fn clip(text: &str, cols: usize) -> String {
    let mut out = String::new();
    let (mut visible, mut cut) = (0, false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // a color code costs no width and is always kept
            out.push(c);
            for d in chars.by_ref() {
                out.push(d);
                if d == 'm' {
                    break;
                }
            }
        } else if visible == cols {
            cut = true;
        } else if !cut {
            visible += 1;
            out.push(c);
        }
    }
    if cut { out + "\x1b[0m" } else { out }
}

/// The visible rows and columns of the terminal behind stderr, 0 and 0 when unknown.
#[cfg(windows)]
pub fn terminal_size() -> (usize, usize) {
    use windows_sys::Win32::System::Console::{CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_ERROR_HANDLE};
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: the standard handle belongs to the process, and info is only written.
    if unsafe { GetConsoleScreenBufferInfo(GetStdHandle(STD_ERROR_HANDLE), &mut info) } == 0 {
        return (0, 0);
    }
    let w = info.srWindow;
    ((w.Bottom - w.Top + 1) as usize, (w.Right - w.Left + 1) as usize)
}

#[cfg(unix)]
pub fn terminal_size() -> (usize, usize) {
    // SAFETY: the ioctl writes only the struct.
    let mut w: libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(libc::STDERR_FILENO, libc::TIOCGWINSZ, &mut w) } != 0 {
        return (0, 0);
    }
    (w.ws_row as usize, w.ws_col as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_color_codes_whole_and_closes_them() {
        assert_eq!(clip("abcdef", 3), "abc\x1b[0m");
        assert_eq!(clip("ab", 3), "ab");
        assert_eq!(clip("\x1b[1mab\x1b[0mcd", 3), "\x1b[1mab\x1b[0mc\x1b[0m");
        assert_eq!(clip("\u{25cf} xy", 2), "\u{25cf} \x1b[0m");
    }
}
