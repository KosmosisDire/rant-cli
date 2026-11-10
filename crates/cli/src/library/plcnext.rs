//! PLCnext Engineer, from the `<name>-<version>.pcwlx` a release carries. A plcx workspace keeps it in
//! libs/ and names it in pcwex.toml, and a project left as its archive takes Engineer's user libraries.
//! Engineer finds a library only by a file named after its title, so a version goes in a folder name.

use std::path::{Path, PathBuf};

use rant_config::paths;
use regex::Regex;

use super::{Kind, Library, Use, icase, read};
use crate::context::Context;
use crate::failure::{Result, failed};
use crate::files;
use crate::github::{Asset, Release};
use crate::packages::{Download, zip};

/// The file that makes a folder a plcx workspace.
pub const WORKSPACE: &str = "pcwex.toml";

pub fn file_name(library: &str, version: &str) -> String {
    format!("{library}-{version}.pcwlx")
}

/// The library's .pcwlx in the release, None when the release has no Engineer library.
pub fn carried<'r>(r: &'r Release, lib: &Library) -> Option<&'r Asset> {
    r.asset(&file_name(&lib.name, &r.version))
}

fn title_of(lib: &Library) -> &str {
    lib.names.plcnext.as_deref().unwrap_or_default()
}

pub fn is_workspace(dir: &Path) -> bool {
    dir.join(WORKSPACE).is_file()
}

/// An Engineer project left as its archive, which takes libraries from Engineer's user libraries.
pub fn is_project(dir: &Path) -> bool {
    let archives = std::fs::read_dir(dir).into_iter().flatten().flatten();
    !is_workspace(dir) && archives.into_iter().any(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("pcwex")))
}

// ---- the user libraries ----

/// The LibraryPath a User.config sets, None when it sets none.
#[cfg(any(windows, test))]
fn library_path(config: &str) -> Option<PathBuf> {
    let start = config.find("<LibraryPath>")? + "<LibraryPath>".len();
    let end = start + config[start..].find("</LibraryPath>")?;
    let p = config[start..end].trim();
    (!p.is_empty()).then(|| PathBuf::from(p))
}

/// The folders Engineer takes user libraries from: the LibraryPath of each version's settings
/// under appdata, else its default under Documents. Empty where Engineer cannot run.
#[cfg(any(windows, test))]
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

/// Whether PLCnext Engineer has run here, by the settings it keeps.
pub fn engineer_found() -> bool {
    crate::home::env_path("APPDATA").is_some_and(|a| a.join("PHOENIX CONTACT").join("PLCnext Engineer").is_dir())
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

/// The file beside a user library copy holding the version rant placed, since a .pcwlx names none.
fn version_file(copy: &Path, lib: &Library) -> PathBuf {
    copy.with_file_name(format!("{}.version", lib.name))
}

/// The copy in Engineer's user libraries as a use of the project in dir. Its version is the one
/// every folder holds, None when they differ or one was placed without rant.
pub fn user_use(dir: &Path, lib: &Library, folders: &[PathBuf]) -> Option<Use> {
    let title = title_of(lib);
    let copies: Vec<PathBuf> = folders.iter().map(|f| f.join(title).join(format!("{title}.pcwlx"))).filter(|c| c.is_file()).collect();
    let first = copies.first()?;
    let versions: Vec<String> = copies.iter().map(|c| read(&version_file(c, lib)).trim().to_string()).collect();
    let mut u = Use::of(lib, Kind::Engineer, "user libraries");
    (u.dir, u.file) = (dir.to_path_buf(), first.clone());
    u.version = versions.iter().all(|v| !v.is_empty() && *v == versions[0]).then(|| versions[0].clone());
    Some(u)
}

/// Copies the release's library into each of Engineer's user library folders as title/title.pcwlx,
/// in place of the copy there, so every project that took it keeps its path. Returns the copies.
pub fn install(ctx: &Context, home: &Path, lib: &Library, r: &Release) -> Result<Vec<PathBuf>> {
    let folders = folders();
    if folders.is_empty() {
        return Err(failed("PLCnext Engineer runs only on Windows, so its user libraries are not here"));
    }
    let download = fetch(ctx, home, lib, r)?;
    let file = download.file().expect("a download");
    let mut placed = Vec::new();
    for f in &folders {
        let dir = f.join(title_of(lib));
        let target = dir.join(format!("{}.pcwlx", title_of(lib)));
        std::fs::create_dir_all(&dir).map_err(|e| failed(format!("cannot create {}: {e}", paths::slash(&dir))))?;
        put(file, &target)?;
        let v = version_file(&target, lib);
        std::fs::write(&v, format!("{}\n", r.version)).map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(&v))))?;
        placed.push(target);
    }
    Ok(placed)
}

/// Downloads the release's library, refused when its title is not the one package.rant names.
fn fetch(ctx: &Context, home: &Path, lib: &Library, r: &Release) -> Result<Download> {
    let name = file_name(&lib.name, &r.version);
    let Some(asset) = carried(r, lib) else { return Err(failed(format!("{} {} carries no {name}", lib.name, r.version))) };
    let download = Download::new(ctx, home, Some(asset))?;
    let title = title(download.file().expect("a download"))?;
    if title != title_of(lib) {
        return Err(failed(format!("{name} holds the Engineer library {title}, not the {} its package.rant names", title_of(lib))));
    }
    Ok(download)
}

/// Copies file to target through a staged file beside it, so a failed copy leaves the old one.
fn put(file: &Path, target: &Path) -> Result<()> {
    let name = target.file_name().expect("a file").to_string_lossy();
    let staging = target.with_file_name(format!(".{name}.{}", files::stamp()));
    if let Err(e) = std::fs::copy(file, &staging).and_then(|_| files::rename(&staging, target)) {
        let _ = std::fs::remove_file(&staging);
        return Err(failed(format!("cannot place {}: {e}, close PLCnext Engineer if a project open there uses it", paths::slash(target))));
    }
    Ok(())
}

// ---- a plcx workspace ----

/// Each [[library]] table of a pcwex.toml as the span of its lines.
fn tables(lines: &[&str]) -> Vec<(usize, usize)> {
    let header = icase(r"^[ \t]*\[");
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].trim() == "[[library]]").collect();
    starts.iter().map(|&s| (s, (s + 1..lines.len()).find(|&i| header.is_match(lines[i])).unwrap_or(lines.len()))).collect()
}

/// A key's line in a table and its value, a TOML string unquoted.
fn value(lines: &[&str], span: (usize, usize), key: &str) -> Option<(usize, String)> {
    let re = Regex::new(&format!(r#"^[ \t]*{key}[ \t]*=[ \t]*(?:'([^']*)'|"((?:[^"\\]|\\.)*)")"#)).expect("a valid pattern");
    (span.0 + 1..span.1).find_map(|i| re.captures(lines[i]).map(|m| (i, m.get(1).map_or_else(|| m[2].replace("\\\\", "\\"), |v| v.as_str().to_string()))))
}

/// The path rant gives the library's file in a workspace: libs\<name>-<version>\<title>.pcwlx.
fn workspace_path(lib: &Library, version: &str) -> String {
    format!(r"libs\{}-{version}\{}.pcwlx", lib.name, title_of(lib))
}

fn own_path(lib: &Library) -> Regex {
    let (name, title) = (regex::escape(&lib.name), regex::escape(title_of(lib)));
    icase(&format!(r"^libs[\\/]{name}-(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)[\\/]{title}\.pcwlx$"))
}

/// The table referencing the library: named by its title, or at the path rant gives it.
fn table(lines: &[&str], lib: &Library) -> Option<(usize, usize)> {
    let own = own_path(lib);
    let names = |s: (usize, usize)| value(lines, s, "name").is_some_and(|(_, n)| n.eq_ignore_ascii_case(title_of(lib)));
    tables(lines).into_iter().find(|&s| names(s) || value(lines, s, "path").is_some_and(|(_, p)| own.is_match(&p)))
}

/// The workspace's reference to the library, with the version its path names.
pub fn workspace_use(text: &str, lib: &Library) -> Option<Use> {
    let normal = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normal.split('\n').collect();
    let span = table(&lines, lib)?;
    let mut u = Use::of(lib, Kind::Plcx, WORKSPACE);
    u.version = value(&lines, span, "path").and_then(|(_, p)| own_path(lib).captures(&p).map(|m| m[1].to_string()));
    Some(u)
}

/// The text with the library's table at version, the table added when there is none. A move drops
/// the table's signature, which named the old file.
pub fn workspace_set(text: &str, lib: &Library, version: &str) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let normal = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normal.split('\n').collect();
    let path = format!("path = '{}'", workspace_path(lib, version));
    let mut out: Vec<String> = Vec::new();
    match table(&lines, lib) {
        Some(span) => {
            let at = value(&lines, span, "path").map(|(i, _)| i);
            let signature = value(&lines, span, "signature").map(|(i, _)| i);
            // a table with no path gets one after its last line
            let last = (span.0..span.1).rev().find(|&i| !lines[i].trim().is_empty()).unwrap_or(span.0);
            for (i, l) in lines.iter().enumerate() {
                if Some(i) == at {
                    out.push(format!("{}{path}", &l[..l.len() - l.trim_start().len()]));
                } else if Some(i) != signature {
                    out.push(l.to_string());
                }
                if at.is_none() && i == last {
                    out.push(format!("  {path}"));
                }
            }
        }
        None => {
            out = lines.iter().map(|l| l.to_string()).collect();
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            out.extend(["".into(), "[[library]]".into(), format!("  name = \"{}\"", title_of(lib)), format!("  {path}"), "".into()]);
        }
    }
    let mut joined = out.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined.replace('\n', nl)
}

/// Puts the release's library in the workspace's libs/ and points its table there, then removes
/// the copy of an older release rant placed.
pub fn into_workspace(ctx: &Context, home: &Path, dir: &Path, lib: &Library, r: &Release) -> Result<()> {
    let config = dir.join(WORKSPACE);
    let text = read(&config);
    let normal = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normal.split('\n').collect();
    let old = table(&lines, lib).and_then(|s| value(&lines, s, "path")).map(|(_, p)| p).filter(|p| own_path(lib).is_match(p));
    let target = dir.join(workspace_path(lib, &r.version).replace('\\', "/"));
    if !target.is_file() {
        let download = fetch(ctx, home, lib, r)?;
        let folder = target.parent().expect("a folder");
        std::fs::create_dir_all(folder).map_err(|e| failed(format!("cannot create {}: {e}", paths::slash(folder))))?;
        put(download.file().expect("a download"), &target)?;
    }
    files::edit(&config, |text| Ok(workspace_set(text, lib, &r.version)))?;
    if let Some(old) = old.map(|p| dir.join(p.replace('\\', "/"))).filter(|p| !paths::same(p, &target)) {
        let _ = std::fs::remove_file(&old);
        let _ = old.parent().map(std::fs::remove_dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::uses::tests::rant;
    use crate::scratch::Scratch;

    fn plcnext() -> Library {
        let mut lib = rant();
        (lib.name, lib.repo) = ("plcnext".into(), "KosmosisDire/rant-plcnext".into());
        lib.names = rant_config::package::Library { plcnext: Some("Rant".into()), ..Default::default() };
        lib
    }

    fn library(t: &Scratch, rel: &str, title: &str) -> PathBuf {
        let core = format!("<coreProperties><dc:title>{title}</dc:title></coreProperties>");
        t.write(rel, zip::zip_of(&[("_properties/Core.xml", &core, true)]))
    }

    const GOLF: &str = "name = \"Golf\"\r\narchive = \"golf.pcwex\"\r\n\r\n[connection]\r\n  ip = \"192.168.1.37\"\r\n\r\n[[library]]\r\n  name = \"PLCnext Controller\"\r\n  path = \"$(SystemLibrariesPath)\\\\PLCnext Controller.pcwlx\"\r\n  id = \"6bdd\"\r\n";

    #[test]
    fn a_workspace_gets_a_table_and_a_move_rewrites_its_path() {
        let lib = plcnext();
        assert_eq!(workspace_use(GOLF, &lib), None);
        let added = workspace_set(GOLF, &lib, "0.0.1");
        assert_eq!(added, format!("{GOLF}\r\n[[library]]\r\n  name = \"Rant\"\r\n  path = 'libs\\plcnext-0.0.1\\Rant.pcwlx'\r\n"));
        assert_eq!(workspace_use(&added, &lib).and_then(|u| u.version).as_deref(), Some("0.0.1"));
        assert_eq!(workspace_set(&added, &lib, "0.0.1"), added);
        let signed = added.replace("pcwlx'\r\n", "pcwlx'\r\n  signature = \"abc\"\r\n");
        let moved = workspace_set(&signed, &lib, "0.0.2");
        assert_eq!(moved, added.replace("0.0.1", "0.0.2"));
    }

    #[test]
    fn a_table_placed_by_hand_is_taken_over() {
        let lib = plcnext();
        let text = "[[library]]\n  name = \"Rant\"\n  path = 'libs\\Rant.pcwlx'\n\n[[library]]\n  name = \"Other\"\n  path = \"x.pcwlx\"\n";
        let u = workspace_use(text, &lib).unwrap();
        assert_eq!((u.how, u.version), (WORKSPACE, None));
        assert_eq!(workspace_set(text, &lib, "0.0.1"), text.replace(r"libs\Rant.pcwlx", r"libs\plcnext-0.0.1\Rant.pcwlx"));
        let pathless = "[[library]]\n  name = \"Rant\"\n";
        assert_eq!(workspace_set(pathless, &lib, "0.0.1"), "[[library]]\n  name = \"Rant\"\n  path = 'libs\\plcnext-0.0.1\\Rant.pcwlx'\n");
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
    fn a_user_copy_has_the_version_every_folder_agrees_on() {
        let t = Scratch::new("pcwlx-user");
        let lib = plcnext();
        let folders = vec![t.path("a"), t.path("b")];
        assert_eq!(user_use(&t.path("p"), &lib, &folders), None);
        library(&t, "a/Rant/Rant.pcwlx", "Rant");
        assert_eq!(user_use(&t.path("p"), &lib, &folders).unwrap().version, None);
        t.write("a/Rant/plcnext.version", "0.0.1\n");
        assert_eq!(user_use(&t.path("p"), &lib, &folders).unwrap().version.as_deref(), Some("0.0.1"));
        library(&t, "b/Rant/Rant.pcwlx", "Rant");
        t.write("b/Rant/plcnext.version", "0.0.2\n");
        assert_eq!(user_use(&t.path("p"), &lib, &folders).unwrap().version, None);
        assert!(title(&library(&t, "bad.pcwlx", "../x")).is_err());
        assert_eq!(title(&library(&t, "good.pcwlx", "Rant")).unwrap(), "Rant");
    }

    #[test]
    fn a_project_is_an_archive_without_a_workspace() {
        let t = Scratch::new("pcwlx-project");
        t.write("p/golf.pcwex", "");
        assert!(is_project(&t.path("p")));
        t.write("p/pcwex.toml", "");
        assert!(!is_project(&t.path("p")) && is_workspace(&t.path("p")));
    }
}
