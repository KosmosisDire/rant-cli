//! PLCnext Engineer libraries: the .pcwlx a release carries, placed where Engineer looks for the
//! user's libraries, each in a folder named after it so every update keeps its path.

use std::path::{Path, PathBuf};

use rant_config::paths;

use super::zip;
use crate::failure::{Result, failed};
use crate::files;

/// The LibraryPath a User.config sets, None when it sets none.
fn library_path(config: &str) -> Option<PathBuf> {
    let start = config.find("<LibraryPath>")? + "<LibraryPath>".len();
    let end = start + config[start..].find("</LibraryPath>")?;
    let p = config[start..end].trim();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

/// The folders Engineer takes user libraries from: the LibraryPath of each version's settings
/// under appdata, else its default under Documents. Empty where Engineer cannot run.
fn folders_in(appdata: Option<PathBuf>, documents: Option<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let versions = appdata.map(|a| a.join("PHOENIX CONTACT").join("PLCnext Engineer"));
    for e in versions.and_then(|v| std::fs::read_dir(v).ok()).into_iter().flatten().flatten() {
        let config = std::fs::read_to_string(e.path().join("User.config")).unwrap_or_default();
        if let Some(p) = library_path(&config)
            && !out.iter().any(|o| paths::same(o, &p))
        {
            out.push(p);
        }
    }
    if out.is_empty()
        && let Some(d) = documents
    {
        out.push(d.join("PLCnext Engineer").join("Libraries"));
    }
    out
}

pub fn folders() -> Vec<PathBuf> {
    #[cfg(windows)]
    return folders_in(crate::home::env_path("APPDATA"), crate::home::documents());
    #[cfg(not(windows))]
    Vec::new()
}

/// The library's own name, the title Engineer shows it by.
pub fn title(file: &Path) -> Result<String> {
    let bytes = std::fs::read(file).map_err(|e| failed(format!("cannot read {}: {e}", paths::slash(file))))?;
    let not = || failed(format!("{} is not a PLCnext Engineer library", paths::slash(file)));
    let core = String::from_utf8(zip::entry(&bytes, "_properties/Core.xml").ok_or_else(not)?).map_err(|_| not())?;
    let start = core.find("<dc:title>").ok_or_else(not)? + "<dc:title>".len();
    let end = start + core[start..].find("</dc:title>").ok_or_else(not)?;
    let name = core[start..end].trim();
    if name.is_empty() || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*']) || name.starts_with('.') {
        return Err(not());
    }
    Ok(name.to_string())
}

/// Copies the library into each folder as name/name.pcwlx in place of the copy there, and
/// returns the files placed.
pub fn install(file: &Path, folders: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let name = title(file)?;
    let mut placed = Vec::new();
    for f in folders {
        let dir = f.join(&name);
        let target = dir.join(format!("{name}.pcwlx"));
        let staging = dir.join(format!(".{name}.pcwlx.{}", files::stamp()));
        let copied = std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(file, &staging)).and_then(|_| files::rename(&staging, &target));
        if let Err(e) = copied {
            let _ = std::fs::remove_file(&staging);
            return Err(failed(format!("cannot place {}: {e}, close PLCnext Engineer if a project open there uses it", paths::slash(&target))));
        }
        placed.push(target);
    }
    Ok(placed)
}

/// Removes libraries placed by install, and each one's folder once nothing else is in it.
pub fn remove(placed: &[PathBuf]) -> Result<()> {
    for p in placed {
        if p.is_file() {
            std::fs::remove_file(p).map_err(|e| failed(format!("cannot remove {}: {e}, close PLCnext Engineer if a project open there uses it", paths::slash(p))))?;
        }
        if let Some(dir) = p.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Ok(())
}

/// The newest library a build folder holds at its top or in its bin, which its build wrote last.
pub fn newest(folder: &Path) -> Option<PathBuf> {
    let candidates = [folder.to_path_buf(), folder.join("bin")].into_iter().flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten());
    let libraries = candidates.filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("pcwlx")));
    libraries.max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok()).map(|e| e.path())
}

/// Whether file is newer than any copy placed from it, or one is missing.
pub fn newer(file: &Path, placed: &[PathBuf]) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    placed.iter().any(|p| modified(p).is_none() || modified(file) > modified(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    fn library(t: &Scratch, rel: &str, title: &str) -> PathBuf {
        let core = format!("<coreProperties><dc:title>{title}</dc:title></coreProperties>");
        let p = t.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, zip::zip_of(&[("_properties/Core.xml", &core, true)])).unwrap();
        p
    }

    #[test]
    fn folders_follow_each_engineer_version() {
        let t = Scratch::new("pcwlx-folders");
        let docs = t.path("docs");
        assert_eq!(folders_in(Some(t.path("appdata")), Some(docs.clone())), vec![docs.join("PLCnext Engineer").join("Libraries")]);
        let config = |p: &Path| format!("<ShellConfiguration>\n  <LibraryPath>{}</LibraryPath>\n</ShellConfiguration>\n", p.display());
        t.write("appdata/PHOENIX CONTACT/PLCnext Engineer/2025.0/User.config", config(&t.path("libs")));
        t.write("appdata/PHOENIX CONTACT/PLCnext Engineer/2026.3/User.config", config(&t.path("libs")));
        t.write("appdata/PHOENIX CONTACT/PLCnext Engineer/2026.6/User.config", config(&t.path("other")));
        let mut found = folders_in(Some(t.path("appdata")), Some(docs));
        found.sort();
        assert_eq!(found, vec![t.path("libs"), t.path("other")]);
    }

    #[test]
    fn install_replaces_and_remove_tidies() {
        let t = Scratch::new("pcwlx-install");
        let file = library(&t, "build/bin/Rant.pcwlx", "Rant");
        assert_eq!(newest(&t.path("build")), Some(file.clone()));
        let into = vec![t.path("libs")];
        let placed = install(&file, &into).unwrap();
        assert_eq!(placed, vec![t.path("libs/Rant/Rant.pcwlx")]);
        assert!(!newer(&file, &placed));
        install(&file, &into).unwrap();
        assert_eq!(std::fs::read_dir(t.path("libs/Rant")).unwrap().count(), 1);
        t.write("libs/Rant/notes.txt", "kept");
        remove(&placed).unwrap();
        assert!(!placed[0].exists() && t.path("libs/Rant/notes.txt").is_file());
        assert!(newer(&file, &placed));
        assert!(title(&library(&t, "bad.pcwlx", "../x")).is_err());
    }
}
