//! Where packages pull in the Rant library and at which version, and the edits that add it
//! or move it to another version. Build files are matched by pattern, never fully parsed,
//! so a file edited by hand still works within reason.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::{Captures, Regex};

use crate::manifest::{RANT_DOTNET_PACKAGE, RANT_PYTHON_PACKAGE};
use crate::model::nearest_venv;
use crate::scan::{imports_rant, skipped_dir};

const RANT_GIT: &str = "https://github.com/KosmosisDire/Rant.git";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    CMake,
    Python,
    CSharp,
}

/// How a package names Rant, which decides whether rant can move its version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Cpm,
    FetchContent,
    FindPackage,
    Pyproject,
    Venv,
    PackageReference,
    ProjectReference,
}

impl How {
    pub fn name(self) -> &'static str {
        match self {
            How::Cpm => "CPM",
            How::FetchContent => "FetchContent",
            How::FindPackage => "find_package",
            How::Pyproject => "pyproject",
            How::Venv => "venv",
            How::PackageReference => "PackageReference",
            How::ProjectReference => "ProjectReference",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    pub kind: Kind,
    pub how: How,
    /// The package folder.
    pub dir: PathBuf,
    /// The build file that names the version, or the venv. Empty for Python with no venv.
    pub file: PathBuf,
    /// None when the file names no version, or the venv lacks Rant.
    pub version: Option<String>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

// ---- CMake ----

/// One command call in a CMakeLists: its name and the text between its parentheses.
#[derive(Debug)]
struct Call {
    name: String,
    /// Just past its closing parenthesis.
    end: usize,
    args: std::ops::Range<usize>,
}

/// The length of a bracket opening such as `[[` or `[==[` at i, and its `=` count.
fn bracket_open(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some((j + 1 - i, j - i - 1))
}

/// Skips a bracket argument or comment opened at i, returning the offset past its close.
fn skip_bracket(text: &str, i: usize, len: usize, eqs: usize) -> usize {
    let close = format!("]{}]", "=".repeat(eqs));
    text[i + len..].find(&close).map(|k| i + len + k + close.len()).unwrap_or(text.len())
}

/// Every command call, skipping comments, quoted and bracket arguments.
fn cmake_calls(text: &str) -> Vec<Call> {
    let b = text.as_bytes();
    let mut calls = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'#' {
            i = match bracket_open(b, i + 1) {
                Some((len, eqs)) => skip_bracket(text, i + 1, len, eqs),
                None => text[i..].find('\n').map(|k| i + k).unwrap_or(b.len()),
            };
            continue;
        }
        if !(c.is_ascii_alphabetic() || c == b'_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let name = text[start..i].to_string();
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if b.get(j) != Some(&b'(') {
            continue;
        }
        let open = j + 1;
        let (mut k, mut depth) = (open, 1);
        while k < b.len() && depth > 0 {
            match b[k] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'"' => {
                    k += 1;
                    while k < b.len() && b[k] != b'"' {
                        k += if b[k] == b'\\' { 2 } else { 1 };
                    }
                }
                b'#' => {
                    k = match bracket_open(b, k + 1) {
                        Some((len, eqs)) => skip_bracket(text, k + 1, len, eqs) - 1,
                        None => text[k..].find('\n').map(|n| k + n).unwrap_or(b.len()),
                    };
                }
                b'[' => {
                    if let Some((len, eqs)) = bracket_open(b, k) {
                        k = skip_bracket(text, k, len, eqs) - 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let end = k.min(b.len());
        calls.push(Call { name, end, args: open..end.saturating_sub(1).max(open) });
        i = end;
    }
    calls
}

fn named(call: &Call, names: &[&str]) -> bool {
    names.iter().any(|n| call.name.eq_ignore_ascii_case(n))
}

fn first_arg(args: &str) -> &str {
    args.trim_start().split(|c: char| c.is_whitespace() || c == ')').next().unwrap_or("").trim_matches('"')
}

/// A fetch of Rant: named rant, or pointing at the Rant repository.
fn fetches_rant(text: &str, call: &Call) -> bool {
    if !named(call, &["CPMAddPackage", "CPMFindPackage", "CPMDeclarePackage", "FetchContent_Declare"]) {
        return false;
    }
    let args = &text[call.args.clone()];
    static NAME: OnceLock<Regex> = OnceLock::new();
    args.to_ascii_lowercase().contains("kosmosisdire/rant")
        || first_arg(args).eq_ignore_ascii_case("rant")
        || re(&NAME, r"(?i)\bNAME\s+rant\b").is_match(args)
}

/// The places a version sits in a fetch: after GIT_TAG or VERSION, after @ in the CPM short
/// form, in a release download or archive URL.
fn version_pattern() -> &'static Regex {
    static V: OnceLock<Regex> = OnceLock::new();
    re(&V, r#"(?i)(GIT_TAG\s+"?|VERSION\s+"?|@|/download/|/archive/(?:refs/tags/)?)(v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z]+)?)"#)
}

fn cmake_use(dir: &Path, file: &Path, text: &str) -> Option<Use> {
    for call in cmake_calls(text) {
        let args = &text[call.args.clone()];
        let how = if fetches_rant(text, &call) {
            if call.name.to_ascii_lowercase().starts_with("cpm") {
                How::Cpm
            } else {
                How::FetchContent
            }
        } else if named(&call, &["find_package"]) && first_arg(args).eq_ignore_ascii_case("rant") {
            How::FindPackage
        } else {
            continue;
        };
        let version = if how == How::FindPackage {
            args.split_whitespace().nth(1).filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())).map(str::to_string)
        } else {
            version_pattern().captures(args).map(|c| c[3].to_string())
        };
        return Some(Use { kind: Kind::CMake, how, dir: dir.to_path_buf(), file: file.to_path_buf(), version });
    }
    None
}

fn cmake_set(text: &str, version: &str) -> Result<String, String> {
    for call in cmake_calls(text) {
        if !fetches_rant(text, &call) {
            continue;
        }
        let args = &text[call.args.clone()];
        if !version_pattern().is_match(args) {
            return Err(format!("its Rant fetch names no version to move, set GIT_TAG v{version} by hand"));
        }
        let moved = version_pattern().replace_all(args, |c: &Captures| format!("{}{}{version}", &c[1], &c[2]));
        return Ok(format!("{}{}{}", &text[..call.args.start], moved, &text[call.args.end..]));
    }
    Err("it uses an installed Rant through find_package, update that install instead".into())
}

/// Adds a fetch of Rant after project() and any CPM setup, with CPM when the file already
/// uses it, else FetchContent. Links the one target when there is exactly one. Returns the
/// new text and, when no target was linked, the line to add by hand.
fn cmake_add(text: &str, version: &str) -> (String, Option<String>) {
    let calls = cmake_calls(text);
    let is_cpm_setup =
        |c: &Call| named(c, &["include"]) && text[c.args.clone()].to_ascii_lowercase().contains("cpm") || named(c, &["CPMAddPackage"]);
    let cpm = calls.iter().any(|c| is_cpm_setup(c));
    let anchor = calls
        .iter()
        .filter(|c| named(c, &["project", "FetchContent_MakeAvailable"]) || is_cpm_setup(c))
        .map(|c| c.end)
        .max();
    let block = if cpm {
        format!("CPMAddPackage(NAME rant\n              GIT_REPOSITORY {RANT_GIT}\n              GIT_TAG v{version})\n")
    } else {
        let include = calls.iter().any(|c| named(c, &["include"]) && first_arg(&text[c.args.clone()]) == "FetchContent");
        format!(
            "{}FetchContent_Declare(rant\n  GIT_REPOSITORY {RANT_GIT}\n  GIT_TAG v{version}\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable(rant)\n",
            if include { "" } else { "include(FetchContent)\n" }
        )
    };

    let targets: Vec<&Call> = calls
        .iter()
        .filter(|c| named(c, &["add_executable", "add_library"]))
        .filter(|c| {
            let args = text[c.args.clone()].to_ascii_uppercase();
            !(args.contains(" IMPORTED") || args.contains(" ALIAS") || args.contains(" INTERFACE"))
        })
        .collect();
    let linked = calls.iter().any(|c| named(c, &["target_link_libraries"]) && text[c.args.clone()].contains("rant::"));
    let link = match (targets.as_slice(), linked) {
        ([one], false) => {
            let name = first_arg(&text[one.args.clone()]).to_string();
            let keyword = calls.iter().filter(|c| named(c, &["target_link_libraries"])).find(|c| first_arg(&text[c.args.clone()]) == name).map(|c| {
                let args = text[c.args.clone()].to_ascii_uppercase();
                ["PRIVATE", "PUBLIC", "INTERFACE"].iter().any(|k| args.split_whitespace().any(|w| w == *k))
            });
            let line = if keyword == Some(false) {
                format!("target_link_libraries({name} rant::rant_host)\n")
            } else {
                format!("target_link_libraries({name} PRIVATE rant::rant_host)\n")
            };
            Some((one.end, line))
        }
        _ => None,
    };

    let mut edits: Vec<(usize, String)> = Vec::new();
    let line_end = |at: usize| text[at..].find('\n').map(|k| at + k + 1).unwrap_or(text.len());
    match anchor {
        Some(at) => edits.push((line_end(at), format!("\n{block}"))),
        None => edits.push((0, format!("{block}\n"))),
    }
    if let Some((at, line)) = &link {
        edits.push((line_end(*at), line.clone()));
    }
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = text.to_string();
    for (at, insert) in edits {
        let insert = if at == out.len() && !out.is_empty() && !out.ends_with('\n') { format!("\n{insert}") } else { insert };
        out.insert_str(at, &insert);
    }
    let hint = link.is_none().then(|| "target_link_libraries(<your target> PRIVATE rant::rant_host)".to_string());
    (out, hint)
}

// ---- Python ----

fn pin_pattern() -> &'static Regex {
    static P: OnceLock<Regex> = OnceLock::new();
    re(&P, r#"(?i)(["']\s*rant[-_.]middleware\s*(?:\[[^\]]*\])?\s*(?:===|==|~=|>=|<=|!=|>|<)\s*)(v?)(\d+(?:\.\d+)*(?:[-.]?[0-9A-Za-z]+)*)"#)
}

fn mentions_rant_dependency(text: &str) -> bool {
    static D: OnceLock<Regex> = OnceLock::new();
    re(&D, r#"(?i)["']\s*rant[-_.]middleware\b"#).is_match(text)
}

/// The version installed in a venv, from its dist-info folder.
pub fn venv_version(venv: &Path) -> Option<String> {
    let mut sites = vec![venv.join("Lib").join("site-packages")];
    if let Ok(entries) = std::fs::read_dir(venv.join("lib")) {
        sites.extend(entries.flatten().map(|e| e.path().join("site-packages")));
    }
    let prefix = format!("{}-", RANT_PYTHON_PACKAGE.replace('-', "_"));
    for site in sites {
        for e in std::fs::read_dir(site).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix(&prefix).and_then(|r| r.strip_suffix(".dist-info")) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// A folder of Python: with explicit, any Python file or a pyproject counts, else only a
/// file that imports rant.
fn holds_python(dir: &Path, explicit: bool) -> bool {
    explicit && dir.join("pyproject.toml").is_file()
        || std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
            let p = e.path();
            p.extension().is_some_and(|x| x == "py") && (explicit || std::fs::read_to_string(&p).is_ok_and(|t| imports_rant(&t)))
        })
}

fn python_uses(dir: &Path, root: Option<&Path>, explicit: bool, out: &mut Vec<Use>) {
    let pyproject = dir.join("pyproject.toml");
    let text = std::fs::read_to_string(&pyproject).unwrap_or_default();
    let listed = mentions_rant_dependency(&text);
    if listed {
        let version = pin_pattern().captures(&text).map(|c| c[3].to_string());
        out.push(Use { kind: Kind::Python, how: How::Pyproject, dir: dir.to_path_buf(), file: pyproject, version });
    }
    if listed || holds_python(dir, explicit) {
        let venv = nearest_venv(dir, root);
        let version = venv.as_deref().and_then(venv_version);
        out.push(Use { kind: Kind::Python, how: How::Venv, dir: dir.to_path_buf(), file: venv.unwrap_or_default(), version });
    }
}

fn pyproject_set(text: &str, version: &str) -> Result<String, String> {
    if !pin_pattern().is_match(text) {
        return Err("its rant-middleware dependency names no version".into());
    }
    Ok(pin_pattern().replace_all(text, |c: &Captures| format!("{}{}{version}", &c[1], &c[2])).into_owned())
}

// ---- C# ----

fn reference_pattern() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, &format!(r#"(?is)<PackageReference\b[^>]*?\bInclude\s*=\s*"{RANT_DOTNET_PACKAGE}"[^>]*?(?:/>|>.*?</PackageReference\s*>)"#))
}

/// A NuGet version as an attribute or a child element, whole: `0.0.16`, `[0.0.16]` or a range.
fn element_version() -> &'static Regex {
    static V: OnceLock<Regex> = OnceLock::new();
    re(&V, r#"(?is)(\bVersion\s*=\s*"|<Version>\s*)([^"<]*?)(\s*(?:"|</Version>))"#)
}

/// The lowest version a NuGet version or range allows: `[0.0.16,)` is 0.0.16.
fn nuget_version(value: &str) -> Option<String> {
    let v = value.trim_matches(|c: char| c == '[' || c == '(' || c.is_whitespace());
    let v = v.split(',').next().unwrap_or("").trim_end_matches([']', ')']).trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// An exact NuGet pin. A plain version means that version or later, and nuget.org has an
/// unrelated package named Rant at 1.0, so a missing feed would quietly build against it.
fn exact(version: &str) -> String {
    format!("[{version}]")
}

fn csharp_use(dir: &Path, file: &Path, text: &str) -> Option<Use> {
    if let Some(m) = reference_pattern().find(text) {
        let version = element_version().captures(m.as_str()).and_then(|c| nuget_version(&c[2]));
        return Some(Use { kind: Kind::CSharp, how: How::PackageReference, dir: dir.to_path_buf(), file: file.to_path_buf(), version });
    }
    static P: OnceLock<Regex> = OnceLock::new();
    let project = re(&P, &format!(r#"(?i)<ProjectReference\s+Include\s*=\s*"[^"]*[/\\]{RANT_DOTNET_PACKAGE}\.csproj""#));
    project.is_match(text).then(|| Use {
        kind: Kind::CSharp,
        how: How::ProjectReference,
        dir: dir.to_path_buf(),
        file: file.to_path_buf(),
        version: None,
    })
}

fn csharp_set(text: &str, version: &str) -> Result<String, String> {
    let Some(m) = reference_pattern().find(text) else {
        return Err("it references the Rant project from source, update that checkout instead".into());
    };
    let element = m.as_str();
    let moved = if element_version().is_match(element) {
        element_version().replace(element, |c: &Captures| format!("{}{}{}", &c[1], exact(version), &c[3])).into_owned()
    } else {
        element.replacen(&format!("\"{RANT_DOTNET_PACKAGE}\""), &format!("\"{RANT_DOTNET_PACKAGE}\" Version=\"{}\"", exact(version)), 1)
    };
    Ok(format!("{}{}{}", &text[..m.start()], moved, &text[m.end()..]))
}

/// Adds a PackageReference beside the project's others, or in a new ItemGroup.
fn csharp_add(text: &str, version: &str) -> Result<String, String> {
    let reference = format!("<PackageReference Include=\"{RANT_DOTNET_PACKAGE}\" Version=\"{}\" />", exact(version));
    static LAST: OnceLock<Regex> = OnceLock::new();
    let last = re(&LAST, r"(?is)\n([ \t]*)<PackageReference\b[^>]*?(?:/>|>.*?</PackageReference\s*>)");
    if let Some(c) = last.captures_iter(text).last() {
        let m = c.get(0).unwrap();
        return Ok(format!("{}\n{}{reference}{}", &text[..m.end()], &c[1], &text[m.end()..]));
    }
    let Some(end) = text.rfind("</Project>") else {
        return Err("it has no </Project> to add to".into());
    };
    Ok(format!("{}  <ItemGroup>\n    {reference}\n  </ItemGroup>\n{}", &text[..end], &text[end..]))
}

// ---- finding and editing ----

/// What a folder itself uses: its CMakeLists, its C# projects, its Python files. A folder
/// named explicitly counts as Python for any Python file, so Rant can be added to it.
pub fn in_folder(dir: &Path, root: Option<&Path>, explicit: bool) -> Vec<Use> {
    let mut out = Vec::new();
    let cmake = dir.join("CMakeLists.txt");
    if let Ok(text) = std::fs::read_to_string(&cmake) {
        out.extend(cmake_use(dir, &cmake, &text));
    }
    let mut projects: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("csproj")))
        .collect();
    projects.sort();
    for p in projects {
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.extend(csharp_use(dir, &p, &text));
        }
    }
    python_uses(dir, root, explicit, &mut out);
    out
}

/// Every use under start by folder, gitignored paths, build trees and venvs left out. A
/// venv shared by several folders is listed once, for the first of them.
pub fn under(start: &Path, root: Option<&Path>) -> Vec<Use> {
    let top = start.to_path_buf();
    let walker = ignore::WalkBuilder::new(start)
        .hidden(false)
        .parents(true)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |e| {
            let p = e.path();
            p == top || !(e.file_type().is_some_and(|t| t.is_dir()) && (skipped_dir(p) || crate::discover::build_tree(p)))
        })
        .build();
    let mut out: Vec<Use> = Vec::new();
    for e in walker.flatten() {
        if !e.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        for u in in_folder(e.path(), root, false) {
            let seen = u.how == How::Venv && !u.file.as_os_str().is_empty() && out.iter().any(|o| o.how == How::Venv && o.file == u.file);
            if !seen {
                out.push(u);
            }
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

fn edit(file: &Path, f: impl FnOnce(&str) -> Result<String, String>) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let new = f(&text)?;
    if new != text {
        std::fs::write(file, new).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    Ok(())
}

/// Moves the Rant version a build file names: a CMakeLists, a pyproject or a C# project.
pub fn set(file: &Path, version: &str) -> Result<(), String> {
    let name = file.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match name.as_str() {
        "cmakelists.txt" => edit(file, |t| cmake_set(t, version)),
        "pyproject.toml" => edit(file, |t| pyproject_set(t, version)),
        n if n.ends_with(".csproj") => edit(file, |t| csharp_set(t, version)),
        _ => Err(format!("{} names no Rant version rant can move", file.display())),
    }
}

/// Adds Rant to a CMakeLists or a C# project. Returns a line still to add by hand, if any.
pub fn add(kind: Kind, file: &Path, version: &str) -> Result<Option<String>, String> {
    let mut hint = None;
    match kind {
        Kind::CMake => edit(file, |t| {
            let (text, h) = cmake_add(t, version);
            hint = h;
            Ok(text)
        })?,
        Kind::CSharp => edit(file, |t| csharp_add(t, version))?,
        Kind::Python => return Err("Python takes Rant into its venv, not its files".into()),
    }
    Ok(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPM: &str = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\ninclude(cmake/get_cpm.cmake)\n# CPMAddPackage(NAME rant GIT_TAG v0.0.1)\nCPMAddPackage(\n  NAME rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG \"v0.0.16\" # pinned\n)\nadd_executable(cam main.cpp)\n";

    #[test]
    fn calls_skip_comments_quotes_and_brackets() {
        let text = "a(x \")\" y) # b(\n#[[ c( ]]\nd([[ ) ]] e)\n";
        let names: Vec<String> = cmake_calls(text).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["a", "d"]);
    }

    #[test]
    fn cpm_version_is_read_and_moved_keeping_the_layout() {
        let u = cmake_use(Path::new("."), Path::new("CMakeLists.txt"), CPM).unwrap();
        assert_eq!((u.how, u.version.as_deref()), (How::Cpm, Some("0.0.16")));
        let moved = cmake_set(CPM, "0.0.18").unwrap();
        assert_eq!(moved, CPM.replace("\"v0.0.16\"", "\"v0.0.18\""), "the commented call stays as it was");
    }

    #[test]
    fn short_forms_and_urls_move_too() {
        let short = "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.16\")\n";
        assert_eq!(cmake_set(short, "0.0.18").unwrap(), "CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.18\")\n");
        let url = "FetchContent_Declare(rant URL https://github.com/KosmosisDire/Rant/archive/refs/tags/v0.0.16.zip)\n";
        assert_eq!(cmake_set(url, "0.0.18").unwrap(), url.replace("0.0.16", "0.0.18"));
        let branch = "FetchContent_Declare(rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG main)\n";
        assert!(cmake_set(branch, "0.0.18").is_err());
        let installed = "find_package(rant 0.0.16 CONFIG REQUIRED)\n";
        let u = cmake_use(Path::new("."), Path::new("x"), installed).unwrap();
        assert_eq!((u.how, u.version.as_deref()), (How::FindPackage, Some("0.0.16")));
        assert!(cmake_set(installed, "0.0.18").is_err());
    }

    #[test]
    fn add_uses_fetchcontent_without_cpm_and_links_the_one_target() {
        let text = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\nadd_executable(cam main.cpp)\n";
        let (out, hint) = cmake_add(text, "0.0.18");
        assert_eq!(hint, None);
        assert_eq!(
            out,
            "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\n\ninclude(FetchContent)\nFetchContent_Declare(rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG v0.0.18\n  GIT_SHALLOW TRUE)\nFetchContent_MakeAvailable(rant)\n\nadd_executable(cam main.cpp)\ntarget_link_libraries(cam PRIVATE rant::rant_host)\n"
        );
        let u = cmake_use(Path::new("."), Path::new("x"), &out).unwrap();
        assert_eq!((u.how, u.version.as_deref()), (How::FetchContent, Some("0.0.18")));
    }

    #[test]
    fn add_follows_cpm_and_the_plain_link_signature() {
        let text = "project(a)\ninclude(cmake/get_cpm.cmake)\nadd_executable(a a.cpp)\ntarget_link_libraries(a m)\n";
        let (out, _) = cmake_add(text, "0.0.18");
        assert!(out.contains("include(cmake/get_cpm.cmake)\n\nCPMAddPackage(NAME rant\n"), "{out}");
        assert!(out.contains("add_executable(a a.cpp)\ntarget_link_libraries(a rant::rant_host)\n"), "{out}");
        let two = "project(a)\nadd_executable(a a.cpp)\nadd_executable(b b.cpp)\n";
        assert!(cmake_add(two, "0.0.18").1.is_some(), "two targets: the user picks");
    }

    #[test]
    fn pyproject_pins_move_and_bare_ones_stay() {
        let text = "[project]\ndependencies = [\"numpy\", \"rant-middleware>=0.0.16\"]\n";
        assert_eq!(pyproject_set(text, "0.0.18").unwrap(), text.replace("0.0.16", "0.0.18"));
        assert!(pyproject_set("dependencies = [\"rant-middleware\"]\n", "0.0.18").is_err());
    }

    #[test]
    fn csharp_reference_is_read_moved_and_added() {
        let text = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"0.0.16\" />\n  </ItemGroup>\n</Project>\n";
        let u = csharp_use(Path::new("."), Path::new("x.csproj"), text).unwrap();
        assert_eq!(u.version.as_deref(), Some("0.0.16"));
        assert_eq!(csharp_set(text, "0.0.18").unwrap(), text.replace("\"0.0.16\"", "\"[0.0.18]\""), "pinned exactly");
        let child = "<PackageReference Include=\"Rant\">\n  <Version> [0.0.16,) </Version>\n</PackageReference>";
        let u = csharp_use(Path::new("."), Path::new("x.csproj"), child).unwrap();
        assert_eq!(u.version.as_deref(), Some("0.0.16"));
        assert_eq!(csharp_set(child, "0.0.18").unwrap(), child.replace("[0.0.16,)", "[0.0.18]"));

        let without = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n  </ItemGroup>\n</Project>\n";
        assert!(csharp_add(without, "0.0.18").unwrap().contains(
            "Version=\"13.0.1\" />\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>"
        ));
        let empty = "<Project Sdk=\"Microsoft.NET.Sdk\">\n</Project>\n";
        assert_eq!(
            csharp_add(empty, "0.0.18").unwrap(),
            "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"[0.0.18]\" />\n  </ItemGroup>\n</Project>\n"
        );
    }

    #[test]
    fn a_folder_lists_its_uses() {
        let t = crate::testdir::TestDir::new();
        t.write("cam/CMakeLists.txt", CPM);
        t.write("py/node.py", "import rant\n");
        t.write("py/.venv/pyvenv.cfg", "home = x\n");
        t.write("py/.venv/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
        t.write("other/readme.md", "x");
        let uses = under(t.root(), None);
        let found: Vec<(How, Option<&str>)> = uses.iter().map(|u| (u.how, u.version.as_deref())).collect();
        assert_eq!(found, [(How::Cpm, Some("0.0.16")), (How::Venv, Some("0.0.17"))]);
    }
}
