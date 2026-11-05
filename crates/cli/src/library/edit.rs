//! Library edits of build files: a use moved to another version, or the library added. Each
//! keeps the rest of the file as written.

use std::path::Path;

use regex::{NoExpand, Regex};

use super::uses::{cargo_entry, cargo_versions, element_version, fetches, is_fetch, pin_pattern, reference_pattern, version_pattern};
use super::{DEFAULT_LIBRARY, Kind, Library};
use crate::cmake::{self, Call};
use crate::failure::{Result, failed};
use crate::files;

fn icase(pattern: &str) -> Regex {
    Regex::new(&format!("(?i){pattern}")).expect("the patterns here are valid")
}

fn git_url(lib: &Library) -> String {
    format!("https://github.com/{}.git", lib.repo)
}

fn names(n: &Option<String>) -> &str {
    n.as_deref().unwrap_or_default()
}

/// Every match of re in text with its third group, the version, made version.
fn with_version(text: &str, re: &Regex, version: &str) -> String {
    let (mut out, mut last) = (String::new(), 0);
    for m in re.captures_iter(text) {
        let v = m.get(3).expect("each version pattern has three groups");
        out += &text[last..v.start()];
        out += version;
        last = v.end();
    }
    out + &text[last..]
}

/// Moves the file's use of the library to version.
pub fn set_version(file: &Path, lib: &Library, version: &str) -> Result<()> {
    let name = file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let set: fn(&str, &Library, &str) -> Result<String> = match name.as_str() {
        "cmakelists.txt" => cmake_set,
        "pyproject.toml" => pyproject_set,
        "cargo.toml" => cargo_set,
        n if n.ends_with(".csproj") => csharp_set,
        _ => return Err(failed(format!("{} names no {} version rant can move", file.display(), lib.name))),
    };
    files::edit(file, |text| set(text, lib, version))
}

/// Adds the library to a build file at version, whose tag is the release's. Returns what to do
/// by hand, empty when nothing is left.
pub fn add(kind: Kind, file: &Path, lib: &Library, version: &str, tag: &str) -> Result<String> {
    let mut hint = String::new();
    match kind {
        Kind::CMake => files::edit(file, |text| {
            let (changed, left) = cmake_add(text, lib, tag)?;
            hint = left;
            Ok(changed)
        })?,
        Kind::CSharp => files::edit(file, |text| csharp_add(text, lib, version))?,
        Kind::Rust => files::edit(file, |text| cargo_add(text, lib, version))?,
        Kind::Python => return Err(failed(format!("Python gets {} in its venv, not its files", lib.name))),
    }
    Ok(hint)
}

// ---- CMake ----

pub fn cmake_set(text: &str, lib: &Library, version: &str) -> Result<String> {
    for c in cmake::calls(text) {
        if !fetches(text, &c, lib) {
            continue;
        }
        let args = c.args(text);
        if !version_pattern().is_match(args) {
            return Err(failed(format!("its {} fetch names no version to move, set GIT_TAG v{version} by hand", lib.name)));
        }
        return Ok(format!("{}{}{}", &text[..c.args_begin], with_version(args, &version_pattern(), version), &text[c.args_end..]));
    }
    Err(failed(format!("it uses an installed {} through find_package, update that install instead", lib.name)))
}

/// Whether a target_link_libraries call's arguments link the library's target.
fn links(args: &str, lib: &Library) -> bool {
    let t = names(&lib.names.cmake);
    let target = match t.find("::") {
        Some(ns) => regex::escape(&t[..ns + 2]),
        None => format!(r"{}(?:[^\w:]|$)", regex::escape(t)),
    };
    Regex::new(&format!(r"(?:^|[^\w:]){target}")).expect("valid").is_match(args)
}

/// The text with the library fetched at tag after the project setup, and linked to the one
/// target when there is one, with what is left to do by hand.
pub fn cmake_add(text: &str, lib: &Library, tag: &str) -> Result<(String, String)> {
    let calls = cmake::calls(text);
    let cpm_setup = |c: &Call| (c.named(&["include"]) && c.args(text).to_lowercase().contains("cpm")) || c.named(&["CPMAddPackage"]);
    let cpm = calls.iter().any(cpm_setup);
    // other libraries may fetch Rant themselves and the first fetch of a name wins, so Rant goes first
    let mut before = text.len();
    if lib.name == DEFAULT_LIBRARY {
        before = calls.iter().filter(|c| is_fetch(c)).map(|c| c.args_begin).fold(before, usize::min);
    }
    let anchor = calls.iter().filter(|c| (c.named(&["project", "FetchContent_MakeAvailable"]) || cpm_setup(c)) && c.end <= before).map(|c| c.end).max();

    let (name, url) = (&lib.name, git_url(lib));
    let block = if cpm {
        format!("CPMAddPackage(NAME {name}\n              GIT_REPOSITORY {url}\n              GIT_TAG {tag})\n")
    } else {
        let included = calls.iter().any(|c| c.named(&["include"]) && cmake::first_arg(c.args(text)) == "FetchContent");
        let include = if included { "" } else { "include(FetchContent)\n" };
        format!("{include}FetchContent_Declare({name}\n  GIT_REPOSITORY {url}\n  GIT_TAG {tag}\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable({name})\n")
    };

    let targets: Vec<&Call> = calls
        .iter()
        .filter(|c| c.named(&["add_executable", "add_library"]))
        .filter(|c| {
            let args = c.args(text).to_uppercase();
            !args.contains(" IMPORTED") && !args.contains(" ALIAS") && !args.contains(" INTERFACE")
        })
        .collect();
    let linked = calls.iter().any(|c| c.named(&["target_link_libraries"]) && links(c.args(text), lib));
    let target = names(&lib.names.cmake);
    let link = (targets.len() == 1 && !linked).then(|| {
        let t = cmake::first_arg(targets[0].args(text));
        // a target linked once without PRIVATE, PUBLIC or INTERFACE must stay without
        let keyword = Regex::new(r"\b(PRIVATE|PUBLIC|INTERFACE)\b").expect("valid");
        let plain = calls.iter().rfind(|c| c.named(&["target_link_libraries"]) && cmake::first_arg(c.args(text)) == t).is_some_and(|c| !keyword.is_match(c.args(text)));
        (targets[0].end, format!("target_link_libraries({t}{}{target})\n", if plain { " " } else { " PRIVATE " }))
    });

    let after_line = |at: usize| text[at..].find('\n').map_or(text.len(), |k| at + k + 1);
    let mut edits = vec![match anchor {
        Some(a) => (after_line(a), format!("\n{block}")),
        None => (0, format!("{block}\n")),
    }];
    if let Some((at, line)) = &link {
        edits.push((after_line(*at), line.clone()));
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    let mut out = text.to_string();
    for (at, insert) in edits {
        let piece = if at == out.len() && !out.is_empty() && !out.ends_with('\n') { format!("\n{insert}") } else { insert };
        out.insert_str(at, &piece);
    }
    let hint = if link.is_some() || linked { String::new() } else { format!("target_link_libraries(<your target> PRIVATE {target})") };
    Ok((out, hint))
}

// ---- Python ----

pub fn pyproject_set(text: &str, lib: &Library, version: &str) -> Result<String> {
    if !pin_pattern(lib).is_match(text) {
        return Err(failed(format!("its {} dependency names no version", names(&lib.names.python))));
    }
    Ok(with_version(text, &pin_pattern(lib), version))
}

// ---- C# ----

/// An exact pin. A plain version means that or later, and nuget.org may hold an unrelated package
/// of the id, as it does Rant 1.0, so a missing feed would quietly build against it.
fn exact(version: &str) -> String {
    format!("[{version}]")
}

pub fn csharp_set(text: &str, lib: &Library, version: &str) -> Result<String> {
    let id = names(&lib.names.csharp);
    let Some(m) = reference_pattern(lib).find(text) else {
        return Err(failed(format!("it references the {id} project from source, update that checkout instead")));
    };
    let element = m.as_str();
    let moved = match element_version().captures(element) {
        Some(v) => {
            let whole = v.get(0).expect("a match");
            format!("{}{}{}{}{}", &element[..whole.start()], &v[1], exact(version), &v[3], &element[whole.end()..])
        }
        None => icase(&format!("\"{}\"", regex::escape(id))).replacen(element, 1, NoExpand(&format!("\"{id}\" Version=\"{}\"", exact(version)))).into_owned(),
    };
    Ok(format!("{}{moved}{}", &text[..m.start()], &text[m.end()..]))
}

pub fn csharp_add(text: &str, lib: &Library, version: &str) -> Result<String> {
    let reference = format!("<PackageReference Include=\"{}\" Version=\"{}\" />", names(&lib.names.csharp), exact(version));
    let last = icase(r"\n([ \t]*)<PackageReference\b[^>]*?(?:/>|>[\s\S]*?</PackageReference\s*>)");
    if let Some(found) = last.captures_iter(text).last() {
        let at = found.get(0).expect("a match").end();
        return Ok(format!("{}\n{}{reference}{}", &text[..at], &found[1], &text[at..]));
    }
    let Some(end) = text.rfind("</Project>") else { return Err(failed("it has no </Project> to add to")) };
    Ok(format!("{}  <ItemGroup>\n    {reference}\n  </ItemGroup>\n{}", &text[..end], &text[end..]))
}

// ---- Rust ----

pub fn cargo_set(text: &str, lib: &Library, version: &str) -> Result<String> {
    let krate = names(&lib.names.rust);
    let Some((start, end)) = cargo_entry(text, lib) else { return Err(failed(format!("it has no {krate} dependency to move"))) };
    let body = &text[start..end];
    if icase(r"\bworkspace\s*=\s*true\b").is_match(body) {
        return Err(failed(format!("it takes {} from its Cargo workspace, move the version in the workspace Cargo.toml", lib.name)));
    }
    if icase(r"\bpath\s*=").is_match(body) {
        return Err(failed(format!("it uses a {} checkout by path, update that checkout instead", lib.name)));
    }
    let re = cargo_versions(lib).into_iter().find(|re| re.is_match(body));
    let Some(re) = re else {
        return Err(failed(format!("its {krate} dependency names no version to move, set tag = \"crate-v{version}\" by hand")));
    };
    Ok(format!("{}{}{}", &text[..start], with_version(body, &re, version), &text[end..]))
}

pub fn cargo_add(text: &str, lib: &Library, version: &str) -> Result<String> {
    if !icase(r"(?:^|\n)\s*\[package\]").is_match(text) {
        return Err(failed(format!("it is a Cargo workspace with no package, add {} to a member", lib.name)));
    }
    // a library's crate-v tag carries what its crate needs packed in, such as Rant's C, so a build needs only cargo
    let line = format!("{} = {{ git = \"{}\", tag = \"crate-v{version}\" }}\n", names(&lib.names.rust), git_url(lib));
    if let Some(m) = icase(r"(?:^|\n)[ \t]*\[dependencies\][ \t]*(?:\r?\n|$)").find(text) {
        let mut head = text[..m.end()].to_string();
        if m.end() == text.len() && !head.is_empty() && !head.ends_with('\n') {
            head.push('\n');
        }
        return Ok(format!("{head}{line}{}", &text[m.end()..]));
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let gap = if out.is_empty() { "" } else { "\n" };
    Ok(format!("{out}{gap}[dependencies]\n{line}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::uses::tests::{CPM, extra, rant};
    use crate::library::uses::{cargo_use, csharp_use};

    fn refused(f: fn(&str, &Library, &str) -> Result<String>, text: &str) -> bool {
        f(text, &rant(), "0.0.18").is_err()
    }

    #[test]
    fn cmake_versions_move_keeping_the_layout() {
        assert_eq!(cmake_set(CPM, &rant(), "0.0.18").unwrap(), CPM.replace("\"v0.0.16\"", "\"v0.0.18\""));
        let shorter = "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.16\")\n";
        assert_eq!(cmake_set(shorter, &rant(), "0.0.18").unwrap(), "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.18\")\n");
        let url = "FetchContent_Declare(rant URL https://github.com/KosmosisDire/Rant/archive/refs/tags/v0.0.16.zip)\n";
        assert_eq!(cmake_set(url, &rant(), "0.0.18").unwrap(), url.replace("0.0.16", "0.0.18"));
        assert!(refused(cmake_set, "FetchContent_Declare(rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG main)\n"));
        assert!(refused(cmake_set, "find_package(rant 0.0.16 CONFIG REQUIRED)\n"));
    }

    #[test]
    fn add_uses_fetchcontent_without_cpm_and_links_the_one_target() {
        let text = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\nadd_executable(cam main.cpp)\n";
        let (out, hint) = cmake_add(text, &rant(), "v0.0.18").unwrap();
        assert_eq!(
            out,
            "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\ninclude(FetchContent)\nFetchContent_Declare(rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG v0.0.18\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable(rant)\n\nadd_executable(cam main.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n"
        );
        assert_eq!(hint, "");
    }

    #[test]
    fn add_follows_cpm_and_the_plain_link_signature() {
        let (out, _) = cmake_add("project(a)\ninclude(cmake/get_cpm.cmake)\nadd_executable(a a.cpp)\ntarget_link_libraries(a m)\n", &rant(), "v0.0.18").unwrap();
        assert!(out.contains("include(cmake/get_cpm.cmake)\n\nCPMAddPackage(NAME rant\n"), "{out}");
        assert!(out.contains("add_executable(a a.cpp)\ntarget_link_libraries(a rant::rant_host)\n"), "{out}");
        let (_, hint) = cmake_add("project(a)\nadd_executable(a a.cpp)\nadd_executable(b b.cpp)\n", &rant(), "v0.0.18").unwrap();
        assert!(!hint.is_empty());
    }

    #[test]
    fn rant_is_fetched_before_the_libraries_that_fetch_it_too() {
        let (after, _) = cmake_add(CPM, &extra(), "v1.2.0").unwrap();
        assert!(after.find("CPMAddPackage(NAME extra") > after.find("NAME rant\n  GIT_REPOSITORY"));
        let text = "project(a)\ninclude(cmake/get_cpm.cmake)\n\n# for poses\nCPMAddPackage(NAME extra GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git GIT_TAG v1.2.0)\nadd_executable(a a.cpp)\n";
        let want = text.replace(
            "include(cmake/get_cpm.cmake)\n",
            "include(cmake/get_cpm.cmake)\n\nCPMAddPackage(NAME rant\n              GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n              GIT_TAG v0.0.18)\n",
        ) + "target_link_libraries(a PRIVATE rant::rant_host)\n";
        assert_eq!(cmake_add(text, &rant(), "v0.0.18").unwrap().0, want);
    }

    #[test]
    fn pyproject_pins_move_and_bare_ones_stay() {
        let text = "[project]\ndependencies = [\"numpy\", \"rant-middleware>=0.0.16\"]\n";
        assert_eq!(pyproject_set(text, &rant(), "0.0.18").unwrap(), text.replace("0.0.16", "0.0.18"));
        assert!(refused(pyproject_set, "dependencies = [\"rant-middleware\"]\n"));
    }

    #[test]
    fn csharp_references_move_pinned_and_are_added() {
        let text = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"0.0.16\" />\n  </ItemGroup>\n</Project>\n";
        assert_eq!(csharp_set(text, &rant(), "0.0.18").unwrap(), text.replace("\"0.0.16\"", "\"[0.0.18]\""));
        let child = "<PackageReference Include=\"Rant\">\n  <Version> [0.0.16,) </Version>\n</PackageReference>";
        assert_eq!(csharp_use(child, &rant()).and_then(|u| u.version).as_deref(), Some("0.0.16"));
        assert_eq!(csharp_set(child, &rant(), "0.0.18").unwrap(), child.replace("[0.0.16,)", "[0.0.18]"));
        let bare = "<PackageReference Include=\"Rant\" />";
        assert_eq!(csharp_set(bare, &rant(), "0.0.18").unwrap(), "<PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />");
        let without = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n  </ItemGroup>\n</Project>\n";
        assert!(csharp_add(without, &rant(), "0.0.18").unwrap().contains("Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>"));
        assert_eq!(
            csharp_add("<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n", &rant(), "0.0.18").unwrap(),
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>\n</Project>\n"
        );
    }

    #[test]
    fn cargo_dependencies_move_and_are_added() {
        let git = "[package]\nname = \"arm\"\n\n[dependencies]\nserde = \"1\"\nrant-middleware = { git = \"https://github.com/KosmosisDire/Rant.git\", tag = \"crate-v0.0.24\" }\n";
        assert_eq!(cargo_set(git, &rant(), "0.0.25").unwrap(), git.replace("crate-v0.0.24", "crate-v0.0.25"));
        let plain_tag = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { git = \"x\", tag = \"v0.0.24\" }\n";
        assert_eq!(cargo_set(plain_tag, &rant(), "0.0.25").unwrap(), plain_tag.replace("\"v0.0.24\"", "\"v0.0.25\""));
        let plain = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = \"=0.0.24\"\nother = \"0.0.24\"\n";
        assert_eq!(cargo_set(plain, &rant(), "0.0.25").unwrap(), plain.replace("rant-middleware = \"=0.0.24\"", "rant-middleware = \"=0.0.25\""));
        let table = "[package]\nname = \"arm\"\n[dependencies.rant-middleware]\ngit = \"x\"\ntag = \"v0.0.24\"\n\n[features]\ntag = \"v9.9.9\"\n";
        assert_eq!(cargo_set(table, &rant(), "0.0.25").unwrap(), table.replace("v0.0.24", "v0.0.25"));
        assert!(refused(cargo_set, "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { workspace = true }\n"));
        assert!(refused(cargo_set, "[package]\nname = \"a\"\n[dependencies]\nrant-middleware = { path = \"../rant\" }\n"));

        let line = "rant-middleware = { git = \"https://github.com/KosmosisDire/Rant.git\", tag = \"crate-v0.0.24\" }\n";
        assert_eq!(cargo_add("[package]\nname = \"arm\"\n\n[dependencies]\nserde = \"1\"\n", &rant(), "0.0.24").unwrap(), format!("[package]\nname = \"arm\"\n\n[dependencies]\n{line}serde = \"1\"\n"));
        assert_eq!(cargo_add("[package]\nname = \"arm\"\n", &rant(), "0.0.24").unwrap(), format!("[package]\nname = \"arm\"\n\n[dependencies]\n{line}"));
        assert!(cargo_add("[workspace]\nmembers = []\n", &rant(), "0.0.24").is_err());
    }

    #[test]
    fn another_library_is_told_apart_by_its_own_names() {
        let linked = "project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n";
        let (added, _) = cmake_add(linked, &extra(), "v1.2.0").unwrap();
        assert!(added.contains("FetchContent_Declare(extra\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git\n  GIT_TAG v1.2.0\n"));
        assert!(added.contains("target_link_libraries(a PRIVATE extra::extra)\n"));
        let py = "dependencies = [\"rant-middleware-extra==1.2.0\", \"rant-middleware>=0.0.16\"]\n";
        assert_eq!(pyproject_set(py, &rant(), "0.0.18").unwrap(), py.replace("0.0.16", "0.0.18"));
        assert_eq!(pyproject_set(py, &extra(), "1.3.0").unwrap(), py.replace("1.2.0", "1.3.0"));
        assert_eq!(
            cargo_add("[package]\nname = \"a\"\n", &extra(), "1.2.0").unwrap(),
            "[package]\nname = \"a\"\n\n[dependencies]\nrant-middleware-extra = { git = \"https://github.com/KosmosisDire/Rant-extra.git\", tag = \"crate-v1.2.0\" }\n"
        );
        assert!(cargo_use("[package]\nname = \"a\"\n[dependencies]\nrant-middleware-extra = { git = \"x\", tag = \"crate-v1.2.0\" }\n", &rant()).is_none());
        let cs = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Rant.Extra\" Version=\"1.2.0\" />\n  </ItemGroup>\n</Project>\n";
        assert_eq!(csharp_set(cs, &extra(), "1.3.0").unwrap(), cs.replace("\"1.2.0\"", "\"[1.3.0]\""));
    }
}
