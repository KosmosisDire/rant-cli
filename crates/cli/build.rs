//! Compiles every file under templates/ into the binary, as the presets of `rant new`, and
//! embeds rant's Windows manifest.

use std::fmt::Write;
use std::path::{Path, PathBuf};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    // byte order of the slashed path, the order a preset writes its files in
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in files {
        writeln!(out, "    ({rel:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("templates.rs");
    std::fs::write(dest, out).unwrap();

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("rant.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-bin=rant=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bin=rant=/MANIFESTINPUT:{}", manifest.display());
    }
}

/// The files under dir, by their path from root. A `.rant` folder is the data an editor
/// keeps when it opens a template as a workspace, never part of one.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != ".rant" {
                collect(root, &path, out);
            }
        } else {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, path));
        }
    }
}
