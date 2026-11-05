//! File helpers the state, the logs and the service process share.

use std::path::Path;
use std::time::{Duration, Instant};

/// A stable name for a path: 16 hex digits of FNV-1a over it, without case on Windows.
pub fn path_key(p: &Path) -> String {
    let text = p.to_string_lossy();
    let text = if cfg!(windows) { text.to_ascii_lowercase() } else { text.into_owned() };
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// Writes a temp file beside the target and renames it into place, so a crash never leaves
/// half a file.
pub fn write_atomic(target: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, text)?;
    rename(Path::new(&tmp), target)
}

/// fs::rename, which Windows refuses while another process holds either side open, as a virus
/// scan holds fresh files for a moment. A refused rename is tried again for up to 3 s.
pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        match std::fs::rename(from, to) {
            // access denied and sharing violation
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32)) && Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(100));
            }
            done => return done,
        }
    }
}

/// What tells one file from another at the same path, None when there is none.
#[cfg(unix)]
pub fn file_id(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(windows)]
pub fn file_id(p: &Path) -> Option<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
    // std shares read, write and delete, so a node may keep writing and rant may move it
    let f = std::fs::File::open(p).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: a live handle and a struct to write.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    Some((info.dwVolumeSerialNumber as u64, ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_key_is_fnv_of_the_path() {
        assert_eq!(path_key(Path::new("")), "cbf29ce484222325");
        assert_eq!(path_key(Path::new("a")), "af63dc4c8601ec8c");
        if cfg!(windows) {
            assert_eq!(path_key(Path::new("C:/Work")), path_key(Path::new("c:/work")));
        }
    }

    #[test]
    fn an_atomic_write_replaces_the_file_and_keeps_its_id_apart() {
        let dir = std::env::temp_dir().join(format!("rant-unit-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("x");
        write_atomic(&f, "one").unwrap();
        let first = file_id(&f).unwrap();
        write_atomic(&f, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "two");
        assert_ne!(file_id(&f).unwrap(), first, "a rename puts another file in place");
        assert_eq!(file_id(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
