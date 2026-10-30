//! Zip files: the VS Code extensions rant reads and the skills packages release, each file
//! stored or deflated.

use std::path::{Path, PathBuf};

use crate::diag::Diag;

/// One file of a zip as its central directory lists it.
struct Entry {
    name: String,
    method: usize,
    local: usize,
    size: usize,
}

/// The files of a zip, None when it is broken.
fn entries(zip: &[u8]) -> Option<Vec<Entry>> {
    let u16_at = |at: usize| zip.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |at: usize| zip.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    // the end record is the last 22 bytes, before a comment of up to 64 KiB
    let last = zip.len().checked_sub(22)?;
    let end = (last.saturating_sub(65535)..=last).rev().find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let mut at = u32_at(end + 16)?;
    let mut out = Vec::new();
    for _ in 0..u16_at(end + 10)? {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let (name_len, extra_len, comment_len) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let name = String::from_utf8(zip.get(at + 46..at + 46 + name_len)?.to_vec()).ok()?;
        // sizes come from the central record, since a streamed local header holds zeros
        out.push(Entry { name, method: u16_at(at + 10)?, local: u32_at(at + 42)?, size: u32_at(at + 20)? });
        at += 46 + name_len + extra_len + comment_len;
    }
    Some(out)
}

/// An entry's bytes, inflated. None when the zip is broken there.
fn data(zip: &[u8], e: &Entry) -> Option<Vec<u8>> {
    let u16_at = |at: usize| zip.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    if zip.get(e.local..e.local + 4)? != 0x0403_4b50u32.to_le_bytes() {
        return None;
    }
    let start = e.local + 30 + u16_at(e.local + 26)? + u16_at(e.local + 28)?;
    let bytes = zip.get(start..start + e.size)?;
    match e.method {
        0 => Some(bytes.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec(bytes).ok(),
        _ => None,
    }
}

/// One file of a zip, inflated. None when the zip is broken or lacks it.
pub fn entry(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    let e = entries(zip)?.into_iter().find(|e| e.name == name)?;
    data(zip, &e)
}

/// A name in a zip as a path below the folder it unpacks into. None for one that could
/// leave it: absolute, with a drive, a backslash, an empty part or a `..`.
fn below(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.strip_suffix('/').unwrap_or(name).split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// Unpacks every file of the zip at file into dest. Checks every name before it writes one.
pub fn extract(file: &Path, dest: &Path) -> Result<(), Diag> {
    let bytes = std::fs::read(file).map_err(|e| Diag::plain(format!("cannot read {}: {e}", file.display())))?;
    let broken = || Diag::plain(format!("{} is a broken zip, or one rant cannot read", file.display()));
    let mut files = Vec::new();
    for e in entries(&bytes).ok_or_else(broken)? {
        let at = below(&e.name).ok_or_else(|| Diag::plain(format!("{} holds `{}`, which lies outside the folder it unpacks into", file.display(), e.name)))?;
        files.push((dest.join(at), e));
    }
    let made = |dir: &Path| std::fs::create_dir_all(dir).map_err(|err| Diag::plain(format!("cannot create {}: {err}", dir.display())));
    made(dest)?;
    for (to, e) in files {
        if e.name.ends_with('/') {
            made(&to)?;
            continue;
        }
        let data = data(&bytes, &e).ok_or_else(broken)?;
        if let Some(dir) = to.parent() {
            made(dir)?;
        }
        std::fs::write(&to, data).map_err(|err| Diag::plain(format!("cannot write {}: {err}", to.display())))?;
    }
    Ok(())
}

/// A zip of these files, deflated or stored, without checksums, which the reader skips.
#[cfg(test)]
pub(crate) fn zip_of(files: &[(&str, &str, bool)]) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, text, deflate) in files {
        let data = if *deflate { miniz_oxide::deflate::compress_to_vec(text.as_bytes(), 6) } else { text.as_bytes().to_vec() };
        let method: u16 = if *deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0]);
        out.extend(method.to_le_bytes());
        out.extend([0; 8]);
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((text.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend([0, 0]);
        out.extend(name.as_bytes());
        out.extend(&data);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend([20, 0, 20, 0, 0, 0]);
        central.extend(method.to_le_bytes());
        central.extend([0; 8]);
        central.extend((data.len() as u32).to_le_bytes());
        central.extend((text.len() as u32).to_le_bytes());
        central.extend((name.len() as u16).to_le_bytes());
        central.extend([0; 12]);
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend([0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn unpacks_each_file_and_folder() {
        let dir = TestDir::new();
        let file = dir.path("skill.zip");
        let files = [("SKILL.md", "---\nname: tool\n---\n", true), ("docs/", "", false), ("docs/more.md", "more", false)];
        std::fs::write(&file, zip_of(&files)).unwrap();
        extract(&file, &dir.path("out")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path("out/SKILL.md")).unwrap(), "---\nname: tool\n---\n");
        assert_eq!(std::fs::read_to_string(dir.path("out/docs/more.md")).unwrap(), "more");
    }

    #[test]
    fn refuses_a_name_outside_the_folder() {
        let dir = TestDir::new();
        for name in ["../up.md", "/root.md", "C:/drive.md", "a\\b.md", "a//b.md", "./a.md"] {
            let file = dir.path("bad.zip");
            std::fs::write(&file, zip_of(&[("SKILL.md", "x", false), (name, "x", false)])).unwrap();
            let err = extract(&file, &dir.path("out")).unwrap_err();
            assert!(err.message.contains("lies outside the folder"), "{name}: {}", err.message);
            assert!(!dir.path("out").exists(), "{name} wrote before the check");
        }
    }

    #[test]
    fn refuses_what_is_no_zip() {
        let dir = TestDir::new();
        std::fs::write(dir.path("text.zip"), "not a zip").unwrap();
        assert!(extract(&dir.path("text.zip"), &dir.path("out")).unwrap_err().message.contains("is a broken zip"));
        assert!(extract(&dir.path("missing.zip"), &dir.path("out")).unwrap_err().message.contains("cannot read"));
    }
}
