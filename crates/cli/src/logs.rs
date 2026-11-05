//! The logs nodes write: <logs>/<name>.log, the run before kept as <name>.log.1.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::files::file_id;

fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

/// The last n lines of a log, fewer when it is shorter, none when it cannot be read.
pub fn tail(log: &Path, n: usize) -> Vec<String> {
    let Ok(mut f) = File::open(log) else { return Vec::new() };
    let size = f.metadata().map_or(0, |m| m.len());
    if n == 0 {
        return Vec::new();
    }
    // reads back from the end, twice as far each time, until n lines are in hand
    let mut window = 64 * 1024u64;
    loop {
        let from = size.saturating_sub(window);
        let mut bytes = Vec::new();
        if f.seek(SeekFrom::Start(from)).is_err() || f.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&bytes);
        // a line cut off at the window's start is dropped
        let whole = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
        let mut lines = split_lines(whole);
        if lines.len() >= n || from == 0 {
            lines.drain(..lines.len().saturating_sub(n));
            return lines;
        }
        window *= 2;
    }
}

/// Every node with a log in the logs folder, by name, a prefixed one as left/lidar.
pub fn logged_nodes(logs: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![logs.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "log") {
                let rel = p.with_extension("");
                let rel = rel.strip_prefix(logs).unwrap_or(&rel);
                out.push(rant_config::paths::slash(rel));
            }
        }
    }
    out.sort();
    out
}

/// A log followed from its end. A restart moves it aside for a new one, told by the file's
/// identity and read from its start. It is never held open, so the move can happen.
pub struct Follower {
    path: PathBuf,
    id: Option<(u64, u64)>,
    offset: u64,
    partial: Vec<u8>,
}

impl Follower {
    pub fn new(path: PathBuf) -> Follower {
        let id = file_id(&path);
        let offset = if id.is_some() { std::fs::metadata(&path).map_or(0, |m| m.len()) } else { 0 };
        Follower { path, id, offset, partial: Vec::new() }
    }

    /// The complete lines written since the last call.
    pub fn poll(&mut self) -> Vec<String> {
        // none between the move and the new log
        let Some(id) = file_id(&self.path) else { return Vec::new() };
        if Some(id) != self.id {
            (self.id, self.offset) = (Some(id), 0);
            self.partial.clear();
        }
        let Ok(mut f) = File::open(&self.path) else { return Vec::new() };
        let size = f.metadata().map_or(0, |m| m.len());
        if size < self.offset {
            // truncated
            self.offset = 0;
            self.partial.clear();
        }
        let mut bytes = Vec::new();
        if size == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() || f.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.extend(bytes);
        let Some(last) = self.partial.iter().rposition(|&b| b == b'\n') else { return Vec::new() };
        let done: Vec<u8> = self.partial.drain(..=last).collect();
        split_lines(&String::from_utf8_lossy(&done))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rant-unit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn the_tail_is_the_last_lines_even_past_one_window() {
        let dir = scratch("logs-tail");
        let log = dir.join("a.log");
        let long: String = (0..20_000).map(|i| format!("line {i}\r\n")).collect();
        std::fs::write(&log, long).unwrap();
        assert_eq!(tail(&log, 2), ["line 19998", "line 19999"]);
        assert_eq!(tail(&log, 15_000).len(), 15_000);
        assert_eq!(tail(&log, 0), Vec::<String>::new());
        std::fs::write(&log, "one\ntwo").unwrap();
        assert_eq!(tail(&log, 5), ["one", "two"]);
        assert!(tail(&dir.join("none.log"), 5).is_empty());
    }

    #[test]
    fn logged_nodes_name_a_prefixed_node_by_its_folder() {
        let dir = scratch("logs-nodes");
        std::fs::create_dir_all(dir.join("left")).unwrap();
        for f in ["talker.log", "talker.log.1", "left/lidar.log", "notes.txt"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        assert_eq!(logged_nodes(&dir), ["left/lidar", "talker"]);
    }

    #[test]
    fn a_follower_reads_new_whole_lines_and_a_restarted_log_from_its_start() {
        let dir = scratch("logs-follow");
        let log = dir.join("a.log");
        std::fs::write(&log, "old\n").unwrap();
        let mut f = Follower::new(log.clone());
        assert!(f.poll().is_empty());
        append(&log, "new\nhal");
        assert_eq!(f.poll(), ["new"]);
        append(&log, "f\n");
        assert_eq!(f.poll(), ["half"]);
        std::fs::rename(&log, dir.join("a.log.1")).unwrap();
        assert!(f.poll().is_empty());
        std::fs::write(&log, "fresh\n").unwrap();
        assert_eq!(f.poll(), ["fresh"]);
    }

    fn append(log: &Path, s: &str) {
        std::io::Write::write_all(&mut std::fs::OpenOptions::new().append(true).open(log).unwrap(), s.as_bytes()).unwrap();
    }
}
