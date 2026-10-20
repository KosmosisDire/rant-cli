//! A VS Code extension file: a zip whose extension/package.json names the extension, which
//! is how rant later finds it in an editor to update or remove it.

use std::path::Path;

use crate::diag::Diag;

#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    /// publisher.name in lowercase, as the editor's command line names it.
    pub id: String,
    pub version: String,
}

pub fn read(path: &Path) -> Result<Extension, Diag> {
    let bytes = std::fs::read(path).map_err(|e| Diag::plain(format!("cannot read {}: {e}", path.display())))?;
    let not = || Diag::plain(format!("{} is not a VS Code extension", path.display()));
    let manifest = entry(&bytes, "extension/package.json").ok_or_else(not)?;
    let json: serde_json::Value = serde_json::from_slice(&manifest).map_err(|_| not())?;
    let field = |key: &str| json.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    match (field("publisher"), field("name"), field("version")) {
        (Some(publisher), Some(name), Some(version)) => {
            Ok(Extension { id: format!("{publisher}.{name}").to_lowercase(), version: version.to_string() })
        }
        _ => Err(not()),
    }
}

/// One file of a zip, inflated. None when the zip is broken or lacks it.
fn entry(zip: &[u8], name: &str) -> Option<Vec<u8>> {
    let u16_at = |at: usize| zip.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |at: usize| zip.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    // the end record is the last 22 bytes, before a comment of up to 64 KiB
    let last = zip.len().checked_sub(22)?;
    let end = (last.saturating_sub(65535)..=last).rev().find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let mut at = u32_at(end + 16)?;
    for _ in 0..u16_at(end + 10)? {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let (method, size) = (u16_at(at + 10)?, u32_at(at + 20)?);
        let (name_len, extra_len, comment_len) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        if zip.get(at + 46..at + 46 + name_len)? == name.as_bytes() {
            // sizes come from the central record, since a streamed local header holds zeros
            let local = u32_at(at + 42)?;
            if u32_at(local)? != 0x0403_4b50 {
                return None;
            }
            let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
            let data = zip.get(start..start + size)?;
            return match method {
                0 => Some(data.to_vec()),
                8 => miniz_oxide::inflate::decompress_to_vec(data).ok(),
                _ => None,
            };
        }
        at += 46 + name_len + extra_len + comment_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    /// A zip of these files, deflated or stored, without checksums, which the reader skips.
    fn zip_of(files: &[(&str, &str, bool)]) -> Vec<u8> {
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

    #[test]
    fn reads_the_id_and_version_an_extension_names() {
        let dir = TestDir::new();
        let manifest = "{ \"name\": \"vscode-rant\", \"publisher\": \"KosmosisDire\", \"version\": \"0.0.9\" }";
        for deflate in [true, false] {
            let file = dir.path("ext.vsix");
            std::fs::write(&file, zip_of(&[("extension.vsixmanifest", "<x/>", deflate), ("extension/package.json", manifest, deflate)])).unwrap();
            assert_eq!(read(&file).unwrap(), Extension { id: "kosmosisdire.vscode-rant".into(), version: "0.0.9".into() });
        }
    }

    #[test]
    fn refuses_what_is_no_extension() {
        let dir = TestDir::new();
        let cases: [(&str, Vec<u8>); 3] = [
            ("text.vsix", b"not a zip".to_vec()),
            ("other.vsix", zip_of(&[("readme.md", "hi", true)])),
            ("unnamed.vsix", zip_of(&[("extension/package.json", "{ \"name\": \"x\", \"version\": \"1\" }", true)])),
        ];
        for (name, bytes) in cases {
            let file = dir.path(name);
            std::fs::write(&file, bytes).unwrap();
            assert!(read(&file).unwrap_err().message.contains("is not a VS Code extension"), "{name}");
        }
        assert!(read(&dir.path("missing.vsix")).unwrap_err().message.contains("cannot read"));
    }
}
