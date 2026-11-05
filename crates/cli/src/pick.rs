//! A checklist asked on stderr: the user ticks any of a few choices with single keys.

use crate::output::{LiveView, Output, Style};

/// One row: a name, and faint words after it such as a description.
#[derive(Debug, Clone)]
pub struct Choice {
    pub label: String,
    pub note: String,
    pub checked: bool,
}

/// The keys a checklist understands. Ctrl-C arrives as Quit, never as a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Toggle,
    All,
    Enter,
    Quit,
    Other,
}

/// A checklist's state, kept apart from the terminal so a test can drive it.
pub struct Checklist {
    title: String,
    choices: Vec<Choice>,
    at: usize,
    ended: bool,
    confirmed: bool,
}

impl Checklist {
    pub fn new(title: &str, choices: Vec<Choice>) -> Checklist {
        Checklist { title: title.to_string(), choices, at: 0, ended: false, confirmed: false }
    }

    /// Moves, ticks or ends. False once Enter or Quit ended it.
    pub fn press(&mut self, k: Key) -> bool {
        let n = self.choices.len();
        match k {
            _ if self.ended || n == 0 => self.ended = true,
            Key::Up => self.at = (self.at + n - 1) % n,
            Key::Down => self.at = (self.at + 1) % n,
            Key::Toggle => self.choices[self.at].checked ^= true,
            Key::All => {
                // all when any is off, else none
                let all = self.choices.iter().all(|c| c.checked);
                self.choices.iter_mut().for_each(|c| c.checked = !all);
            }
            Key::Enter => (self.ended, self.confirmed) = (true, true),
            Key::Quit => self.ended = true,
            Key::Other => {}
        }
        !self.ended
    }

    pub fn ticked(&self) -> Vec<usize> {
        (0..self.choices.len()).filter(|&i| self.choices[i].checked).collect()
    }

    /// The frame to draw: the title, a line of keys, then a row per choice, the current one
    /// marked. Once ended, one line with what was picked.
    pub fn lines(&self, out: &Output) -> Vec<String> {
        let mark = format!("{} {}", out.paint_err(Style::Accent, "?"), out.paint_err(Style::Bold, &self.title));
        if self.ended {
            let picked: Vec<&str> = self.ticked().into_iter().map(|i| self.choices[i].label.as_str()).collect();
            let picked = if !self.confirmed { "cancelled".to_string() } else if picked.is_empty() { "none".into() } else { picked.join(", ") };
            return vec![format!("{mark}  {}", out.paint_err(Style::Dim, &picked))];
        }
        let keys = out.paint_err(Style::Faint, "  up and down move, space ticks, a ticks all, enter confirms, q cancels");
        let width = self.choices.iter().map(|c| c.label.chars().count()).max().unwrap_or(0);
        let mut lines = vec![mark, keys];
        for (i, c) in self.choices.iter().enumerate() {
            let label = format!("{:width$}", c.label);
            let (lead, label) = if i == self.at { (out.paint_err(Style::Accent, "> "), out.paint_err(Style::Accent, &label)) } else { ("  ".into(), label) };
            let tick = if c.checked { "[x] " } else { "[ ] " };
            lines.push(format!("{lead}{tick}{label}  {}", out.paint_err(Style::Faint, &c.note)));
        }
        lines
    }
}

/// Asks with a checklist. The ticked indexes, or None when it was given up or stdin or stderr
/// is no terminal.
pub fn pick(out: &Output, title: &str, choices: Vec<Choice>) -> Option<Vec<usize>> {
    if choices.is_empty() || !out.terminal_err() {
        return None;
    }
    let keys = keys::Reader::new()?;
    let mut list = Checklist::new(title, choices);
    let mut view = LiveView::new();
    view.draw(&list.lines(out));
    while list.press(keys.next()) {
        view.draw(&list.lines(out));
    }
    view.draw(&list.lines(out));
    drop(view);
    list.confirmed.then(|| list.ticked())
}

/// Single keys from the terminal on stdin, without echo or line editing, until the Reader
/// drops and the terminal is as it was.
#[cfg(windows)]
mod keys {
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetStdHandle,
        INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, STD_INPUT_HANDLE, SetConsoleMode,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_UP};

    use super::{Key, from_char};

    pub struct Reader {
        saved: u32,
    }

    impl Reader {
        pub fn new() -> Option<Reader> {
            let mut mode = 0;
            // SAFETY: the standard handle belongs to the process and only its mode changes.
            unsafe {
                let input = GetStdHandle(STD_INPUT_HANDLE);
                if GetConsoleMode(input, &mut mode) == 0 {
                    return None;
                }
                // virtual key codes rather than escape sequences, and Ctrl-C as a key
                let raw = mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
                (SetConsoleMode(input, raw) != 0).then_some(Reader { saved: mode })
            }
        }

        /// Waits for the next key. A closed stdin reads as Quit.
        pub fn next(&self) -> Key {
            loop {
                let mut rec = INPUT_RECORD::default();
                let mut n = 0;
                // SAFETY: one record to write, as counted.
                if unsafe { ReadConsoleInputW(GetStdHandle(STD_INPUT_HANDLE), &mut rec, 1, &mut n) } == 0 || n == 0 {
                    return Key::Quit;
                }
                if rec.EventType != KEY_EVENT as u16 {
                    continue;
                }
                // SAFETY: a key event, as its type says.
                let k = unsafe { rec.Event.KeyEvent };
                if k.bKeyDown == 0 {
                    continue;
                }
                match k.wVirtualKeyCode {
                    VK_UP => return Key::Up,
                    VK_DOWN => return Key::Down,
                    _ => {}
                }
                // SAFETY: the union holds the UTF-16 unit for a key event.
                let c = unsafe { k.uChar.UnicodeChar };
                if c != 0 {
                    return from_char(c as u32);
                }
            }
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            // SAFETY: as in new
            unsafe { SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), self.saved) };
        }
    }
}

#[cfg(unix)]
mod keys {
    use super::{Key, from_char};

    pub struct Reader {
        saved: libc::termios,
    }

    impl Reader {
        pub fn new() -> Option<Reader> {
            // SAFETY: termios calls on stdin with structs they fill.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                if libc::isatty(libc::STDIN_FILENO) == 0 || libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
                    return None;
                }
                let saved = t;
                t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG);
                t.c_cc[libc::VMIN] = 1;
                t.c_cc[libc::VTIME] = 0;
                (libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) == 0).then_some(Reader { saved })
            }
        }

        fn byte(wait_ms: i32) -> Option<u8> {
            let mut p = libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 };
            let mut c = 0u8;
            // SAFETY: one pollfd and one byte, as counted.
            unsafe { (wait_ms < 0 || libc::poll(&mut p, 1, wait_ms) > 0) && libc::read(libc::STDIN_FILENO, (&mut c as *mut u8).cast(), 1) == 1 }.then_some(c)
        }

        /// Waits for the next key. A closed stdin reads as Quit, and an arrow is an escape
        /// sequence whose bytes follow within 50 ms.
        pub fn next(&self) -> Key {
            match Reader::byte(-1) {
                None => Key::Quit,
                Some(27) => match Reader::byte(50) {
                    None => Key::Quit,
                    Some(b'[' | b'O') => match Reader::byte(50) {
                        Some(b'A') => Key::Up,
                        Some(b'B') => Key::Down,
                        _ => Key::Other,
                    },
                    Some(_) => Key::Other,
                },
                Some(c) => from_char(c as u32),
            }
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            // SAFETY: restores what new saved.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.saved) };
        }
    }
}

fn from_char(c: u32) -> Key {
    match char::from_u32(c).unwrap_or('\0') {
        'k' => Key::Up,
        'j' => Key::Down,
        ' ' => Key::Toggle,
        'a' => Key::All,
        '\r' | '\n' => Key::Enter,
        // Ctrl-C, Ctrl-D and Esc
        'q' | '\u{3}' | '\u{4}' | '\u{1b}' => Key::Quit,
        _ => Key::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Format;

    fn choices() -> Vec<Choice> {
        ["a", "bb", "c"].iter().map(|l| Choice { label: l.to_string(), note: String::new(), checked: false }).collect()
    }

    #[test]
    fn keys_move_tick_and_end_a_checklist() {
        let mut list = Checklist::new("Pick", choices());
        for k in [Key::Down, Key::Toggle, Key::Up, Key::Up, Key::Toggle] {
            assert!(list.press(k));
        }
        assert_eq!(list.ticked(), [1, 2]);
        list.press(Key::All);
        assert_eq!(list.ticked(), [0, 1, 2]);
        list.press(Key::All);
        assert!(list.ticked().is_empty());
        assert!(!list.press(Key::Enter) && list.confirmed);
        assert!(!list.press(Key::Toggle), "an ended list takes no more keys");
    }

    #[test]
    fn an_ended_checklist_is_one_line_naming_the_picks() {
        let out = Output::new(Format::Text, false);
        let mut list = Checklist::new("Stop in", choices());
        assert_eq!(list.lines(&out).len(), 5);
        list.press(Key::Toggle);
        list.press(Key::Enter);
        assert!(list.lines(&out)[0].ends_with("  a"));
        let mut quit = Checklist::new("Stop in", choices());
        quit.press(Key::Quit);
        assert!(quit.lines(&out)[0].ends_with("cancelled"));
    }
}
