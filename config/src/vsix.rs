//! A VS Code extension file: a zip whose extension/package.json names the extension, which
//! is how rant later finds it in an editor to update or remove it.

use std::path::Path;

use crate::diag::Diag;
use crate::zip::entry;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use crate::zip::zip_of;

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
