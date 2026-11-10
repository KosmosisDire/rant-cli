//! Arduino and PlatformIO, both from the `<name>-<version>-arduino.zip` a release carries. PlatformIO
//! pins its URL in platformio.ini, and an Arduino sketch takes the library unpacked into the sketchbook.

use std::path::{Path, PathBuf};

use rant_config::paths;
use regex::Regex;

use super::{Kind, Library, Use};
use crate::context::Context;
use crate::failure::{Result, failed};
use crate::files;
use crate::github::{Asset, Release};
use crate::home::{env_path, home_dir};
use crate::packages::{Download, is_link, unlink, zip};

fn icase(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("the patterns here are valid")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

pub fn zip_name(library: &str, version: &str) -> String {
    format!("{library}-{version}-arduino.zip")
}

/// The library's zip in the release, None when the release has no Arduino library.
pub fn carried<'r>(r: &'r Release, lib: &Library) -> Option<&'r Asset> {
    r.asset(&zip_name(&lib.name, &r.version))
}

/// A folder holding `<folder>.ino`, as the Arduino IDE requires of a sketch.
pub fn is_sketch(dir: &Path) -> bool {
    dir.file_name().is_some_and(|n| dir.join(format!("{}.ino", n.to_string_lossy())).is_file())
}

// ---- the sketchbook ----

/// The `user` folder under `directories` in an arduino-cli.yaml.
fn yaml_user(text: &str) -> Option<PathBuf> {
    let mut inside = false;
    for line in text.lines() {
        if !line.starts_with([' ', '\t']) {
            inside = line.trim_end() == "directories:";
            continue;
        }
        if let Some(v) = line.trim().strip_prefix("user:").map(|v| v.trim().trim_matches(['"', '\''])).filter(|v| inside && !v.is_empty()) {
            return Some(PathBuf::from(v));
        }
    }
    None
}

/// The Arduino IDE's sketchbook: what arduino-cli reads from ARDUINO_DIRECTORIES_USER or the IDE's
/// arduino-cli.yaml, else its default for this system.
pub fn sketchbook() -> Option<PathBuf> {
    if let Some(p) = env_path("ARDUINO_DIRECTORIES_USER") {
        return Some(p);
    }
    let home = home_dir()?;
    if let Some(p) = yaml_user(&read(&home.join(".arduinoIDE").join("arduino-cli.yaml"))) {
        return Some(p);
    }
    #[cfg(windows)]
    return crate::home::documents().map(|d| d.join("Arduino"));
    #[cfg(target_os = "macos")]
    return Some(home.join("Documents").join("Arduino"));
    #[cfg(not(any(windows, target_os = "macos")))]
    Some(home.join("Arduino"))
}

/// A field of a library.properties.
fn property(text: &str, key: &str) -> Option<String> {
    let re = icase(&format!(r"(?m)^[ \t]*{key}[ \t]*=[ \t]*(.*?)[ \t]*\r?$"));
    re.captures(text).map(|m| m[1].to_string()).filter(|v| !v.is_empty())
}

/// Whether a library.properties is the library's own: its url names the library's repo.
fn is_own(properties: &str, lib: &Library) -> bool {
    let url = property(properties, "url").unwrap_or_default();
    let url = url.trim_end_matches('/');
    url.strip_suffix(".git").unwrap_or(url).eq_ignore_ascii_case(&format!("https://github.com/{}", lib.repo))
}

/// The folder in the sketchbook holding the library, with its version.
fn installed(libraries: &Path, lib: &Library) -> Option<(PathBuf, Option<String>)> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(libraries).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.into_iter().find_map(|d| {
        let text = read(&d.join("library.properties"));
        is_own(&text, lib).then(|| (d, property(&text, "version")))
    })
}

/// The sketchbook's copy of the library as a use of the sketch in dir, or of the sketchbook itself.
pub fn sketchbook_use(dir: &Path, lib: &Library) -> Option<Use> {
    let (folder, version) = installed(&sketchbook()?.join("libraries"), lib)?;
    let mut u = Use::of(lib, Kind::Arduino, "sketchbook");
    (u.dir, u.file, u.version) = (dir.to_path_buf(), folder, version);
    Some(u)
}

/// Unpacks the release's library into the sketchbook in place of the copy there. Returns its folder.
pub fn install(ctx: &Context, home: &Path, lib: &Library, r: &Release) -> Result<PathBuf> {
    let name = zip_name(&lib.name, &r.version);
    let Some(asset) = carried(r, lib) else { return Err(failed(format!("{} {} carries no {name}", lib.name, r.version))) };
    let Some(book) = sketchbook() else { return Err(failed("cannot tell where the Arduino sketchbook is, set ARDUINO_DIRECTORIES_USER")) };
    let libraries = book.join("libraries");
    let download = Download::new(ctx, home, Some(asset))?;
    let staging = libraries.join(format!(".rant-{}", files::stamp()));
    let placed = place(download.file().expect("a download"), &staging, &libraries, lib, &name);
    let _ = std::fs::remove_dir_all(&staging);
    placed
}

fn place(zip_file: &Path, staging: &Path, libraries: &Path, lib: &Library, name: &str) -> Result<PathBuf> {
    zip::extract(zip_file, staging)?;
    let top: Vec<PathBuf> = std::fs::read_dir(staging).into_iter().flatten().flatten().map(|e| e.path()).collect();
    let [unpacked] = top.as_slice() else { return Err(failed(format!("{name} holds more than one folder at its top"))) };
    if !is_own(&read(&unpacked.join("library.properties")), lib) {
        return Err(failed(format!("{name} is no Arduino library of {}, its library.properties names another url", lib.repo)));
    }
    let target = libraries.join(unpacked.file_name().expect("a folder"));
    if target.exists() && !is_own(&read(&target.join("library.properties")), lib) {
        return Err(failed(format!("{} holds another library, so rant leaves it", paths::slash(&target))));
    }
    // an older release may have unpacked under another folder name
    let old = installed(libraries, lib).map(|(d, _)| d);
    for d in old.iter().chain([&target]).filter(|d| d.exists()) {
        let removed = if is_link(d) { unlink(d) } else { std::fs::remove_dir_all(d).map_err(|e| failed(format!("cannot remove {}: {e}", paths::slash(d)))) };
        removed?;
    }
    std::fs::create_dir_all(libraries).map_err(|e| failed(format!("cannot create {}: {e}", paths::slash(libraries))))?;
    std::fs::rename(unpacked, &target).map_err(|e| failed(format!("cannot move {} into {}: {e}", lib.name, paths::slash(libraries))))?;
    Ok(target)
}

// ---- PlatformIO ----

fn zip_pattern(lib: &Library) -> Regex {
    let version = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?";
    icase(&format!(r"(github\.com/{}/releases/download/)(v?)({version})(/{}-)({version})(-arduino\.zip)", regex::escape(&lib.repo), regex::escape(&lib.name)))
}

pub fn zip_url(lib: &Library, tag: &str, version: &str) -> String {
    format!("https://github.com/{}/releases/download/{tag}/{}", lib.repo, zip_name(&lib.name, version))
}

pub fn pio_use(text: &str, lib: &Library) -> Option<Use> {
    let m = zip_pattern(lib).captures(text)?;
    let mut u = Use::of(lib, Kind::PlatformIO, "lib_deps");
    u.version = Some(m[5].to_string());
    Some(u)
}

pub fn pio_set(text: &str, lib: &Library, version: &str) -> Result<String> {
    let re = zip_pattern(lib);
    if !re.is_match(text) {
        return Err(failed(format!("its lib_deps names no {} release to move", lib.name)));
    }
    Ok(re.replace_all(text, |m: &regex::Captures| format!("{}{}{version}{}{version}{}", &m[1], &m[2], &m[4], &m[6])).into_owned())
}

/// A key's lines in a section: the key line and the last of its indented continuations.
fn key_lines(lines: &[&str], section: (usize, usize), key: &str) -> Option<(usize, usize)> {
    let re = icase(&format!(r"^[ \t]*{key}[ \t]*="));
    let k = (section.0 + 1..section.1).find(|&i| re.is_match(lines[i]))?;
    let more = |i: usize| i < section.1 && lines[i].starts_with([' ', '\t']) && !lines[i].trim().is_empty();
    let mut last = k;
    while more(last + 1) {
        last += 1;
    }
    Some((k, last))
}

/// The sections lib_deps goes in: [env], which every env takes, else each [env:name].
fn env_sections(lines: &[&str]) -> Vec<(usize, usize)> {
    let header = icase(r"^[ \t]*\[([^\]]*)\]");
    let heads: Vec<(usize, String)> = lines.iter().enumerate().filter_map(|(i, l)| header.captures(l).map(|m| (i, m[1].trim().to_lowercase()))).collect();
    let spans: Vec<(usize, usize, &str)> = heads.iter().enumerate().map(|(n, (i, name))| (*i, heads.get(n + 1).map_or(lines.len(), |h| h.0), name.as_str())).collect();
    let common: Vec<(usize, usize)> = spans.iter().filter(|s| s.2 == "env").map(|s| (s.0, s.1)).collect();
    if !common.is_empty() {
        return common;
    }
    spans.iter().filter(|s| s.2.starts_with("env:")).map(|s| (s.0, s.1)).collect()
}

/// Adds value to a key of the section, as one more indented line, or the key with it when missing.
fn append(lines: &[&str], section: (usize, usize), key: &str, value: &str, edits: &mut Vec<(usize, String)>) {
    match key_lines(lines, section, key) {
        Some((k, last)) => {
            let indent = if last > k { lines[last][..lines[last].len() - lines[last].trim_start().len()].to_string() } else { "    ".into() };
            edits.push((last + 1, format!("{indent}{value}")));
        }
        None => {
            let end = (section.0 + 1..section.1).rev().find(|&i| !lines[i].trim().is_empty()).map_or(section.0 + 1, |i| i + 1);
            edits.push((end, format!("{key} =\n    {value}")));
        }
    }
}

fn value_of(lines: &[&str], section: (usize, usize), key: &str) -> String {
    key_lines(lines, section, key).map(|(k, last)| lines[k..=last].join("\n")).unwrap_or_default()
}

/// The text with the library's zip at url in lib_deps, and C++17 for envs that set no standard,
/// since the stock ESP32 platform builds gnu++11 and Rant's C++ needs 17.
pub fn pio_add(text: &str, url: &str) -> Result<String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let normal = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normal.split('\n').collect();
    let sections = env_sections(&lines);
    if sections.is_empty() {
        return Err(failed("it has no [env] section to add to"));
    }
    let mut edits = Vec::new();
    for &s in &sections {
        append(&lines, s, "lib_deps", url, &mut edits);
        if !value_of(&lines, s, "build_flags").contains("-std=") {
            if !value_of(&lines, s, "build_unflags").contains("-std=gnu++11") {
                append(&lines, s, "build_unflags", "-std=gnu++11", &mut edits);
            }
            append(&lines, s, "build_flags", "-std=gnu++17", &mut edits);
        }
    }
    // each edit goes before a line, the later ones first, ties kept in order
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse((edits[i].0, i)));
    for i in order {
        let (at, insert) = &edits[i];
        // a file with no last newline splits into a last line that is text
        let at = if *at == out.len() && out.last().is_some_and(|l| l.is_empty()) { at - 1 } else { *at };
        out.insert(at, insert.clone());
    }
    let mut joined = out.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    Ok(joined.replace('\n', nl))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::uses::tests::{extra, rant};
    use crate::scratch::Scratch;

    const URL: &str = "https://github.com/KosmosisDire/Rant/releases/download/v0.0.43/rant-0.0.43-arduino.zip";

    #[test]
    fn the_ide_yaml_names_the_sketchbook() {
        let text = "board_manager:\n    additional_urls: []\ndirectories:\n    data: C:\\data\n    user: \"D:\\Sketches\"\n";
        assert_eq!(yaml_user(text), Some(PathBuf::from("D:\\Sketches")));
        assert_eq!(yaml_user("board_manager:\n    user: x\n"), None);
    }

    #[test]
    fn a_library_is_known_by_the_url_of_its_repo() {
        assert!(is_own("name=Rant\nversion=0.0.43\nurl=https://github.com/KosmosisDire/Rant\n", &rant()));
        assert!(is_own("url = https://github.com/kosmosisdire/rant.git/\r\n", &rant()));
        assert!(!is_own("url=https://github.com/KosmosisDire/Rant-extra\n", &rant()));
        assert_eq!(property("name=Rant\r\nversion = 0.0.43 \r\n", "version").as_deref(), Some("0.0.43"));
    }

    fn zip_at(t: &Scratch, rel: &str, top: &str, url: &str, version: &str) -> PathBuf {
        let properties = format!("name=Rant
version={version}
url={url}
");
        let entries = [(format!("{top}/library.properties"), properties.as_str(), true), (format!("{top}/src/rant.h"), "/* rant */", false)];
        t.write(rel, zip::zip_of(&entries.iter().map(|(n, s, d)| (n.as_str(), *s, *d)).collect::<Vec<_>>()))
    }

    #[test]
    fn the_sketchbook_copy_is_replaced_and_another_library_kept() {
        let t = Scratch::new("sketchbook");
        let (libraries, own) = (t.path("book/libraries"), "https://github.com/KosmosisDire/Rant");
        let first = zip_at(&t, "a.zip", "Rant", own, "0.0.42");
        assert_eq!(place(&first, &t.path("stage1"), &libraries, &rant(), "a.zip").unwrap(), libraries.join("Rant"));
        assert_eq!(installed(&libraries, &rant()).and_then(|(_, v)| v).as_deref(), Some("0.0.42"));
        // a later release under another folder name takes the old one's place
        let renamed = zip_at(&t, "b.zip", "Rant_Lib", own, "0.0.43");
        place(&renamed, &t.path("stage2"), &libraries, &rant(), "b.zip").unwrap();
        assert!(!libraries.join("Rant").exists());
        assert_eq!(installed(&libraries, &rant()), Some((libraries.join("Rant_Lib"), Some("0.0.43".into()))));
        t.write("book/libraries/Mine/library.properties", "name=Mine
url=https://example.com/mine
");
        let clash = zip_at(&t, "c.zip", "Mine", own, "0.0.44");
        assert!(place(&clash, &t.path("stage3"), &libraries, &rant(), "c.zip").is_err());
        assert!(libraries.join("Mine/library.properties").is_file());
        let foreign = zip_at(&t, "d.zip", "Rant", "https://github.com/someone/else", "1.0.0");
        assert!(place(&foreign, &t.path("stage4"), &libraries, &rant(), "d.zip").is_err());
    }

    #[test]
    fn a_pinned_zip_is_read_and_moved() {
        let text = format!("[env:s3]\nlib_deps =\n    fastled/FastLED\n    {URL}\n");
        assert_eq!(pio_use(&text, &rant()).and_then(|u| u.version).as_deref(), Some("0.0.43"));
        assert_eq!(pio_use(&text, &extra()), None);
        assert_eq!(pio_set(&text, &rant(), "0.0.44").unwrap(), text.replace("0.0.43", "0.0.44"));
        assert!(pio_set("[env:s3]\n", &rant(), "0.0.44").is_err());
    }

    #[test]
    fn the_zip_joins_lib_deps_with_cpp17() {
        let text = "[platformio]\ndefault_envs = s3\n\n[env:s3]\nplatform = espressif32\nlib_deps =\n    fastled/FastLED\n\nmonitor_speed = 115200\n";
        let want = format!(
            "[platformio]\ndefault_envs = s3\n\n[env:s3]\nplatform = espressif32\nlib_deps =\n    fastled/FastLED\n    {URL}\n\nmonitor_speed = 115200\nbuild_unflags =\n    -std=gnu++11\nbuild_flags =\n    -std=gnu++17\n"
        );
        assert_eq!(pio_add(text, URL).unwrap(), want);
    }

    #[test]
    fn a_standard_set_by_hand_stays_and_env_is_shared() {
        let text = "[env]\nframework = arduino\nbuild_flags = -std=gnu++2b\n[env:a]\nboard = x\n[env:b]\nboard = y";
        assert_eq!(pio_add(text, URL).unwrap(), format!("[env]\nframework = arduino\nbuild_flags = -std=gnu++2b\nlib_deps =\n    {URL}\n[env:a]\nboard = x\n[env:b]\nboard = y\n"));
        let each = "[env:a]\r\nboard = x\r\n\r\n[env:b]\r\nboard = y\r\nbuild_unflags = -std=gnu++11\r\n";
        let out = pio_add(each, URL).unwrap();
        assert_eq!(out.matches(URL).count(), 2);
        assert_eq!(out.matches("-std=gnu++11").count(), 2, "{out}");
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
        assert!(pio_add("[platformio]\n", URL).is_err());
    }
}
