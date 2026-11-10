//! Where packages take each library from and at which version. Build files are matched by
//! pattern, never parsed, so hand edits work within reason.

use std::cell::LazyCell;
use std::path::{Path, PathBuf};

use rant_config::discover;
use rant_config::model::NodeType;
use rant_config::python::{self, Folder};
use rant_config::scan::NodeKind;
use regex::Regex;

use super::arduino::Book;
use super::{DEFAULT_LIBRARY, Kind, Library, arduino, icase, read};
use crate::cmake;

/// One place a package takes a library from. how says which, such as "CPM", "venv" or "Cargo
/// workspace", and so whether rant can move it.
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    pub library: String,
    pub kind: Kind,
    pub how: &'static str,
    /// The package folder.
    pub dir: PathBuf,
    /// The build file, or the venv, which may not exist yet.
    pub file: PathBuf,
    /// None when not named, or not in the venv.
    pub version: Option<String>,
    pub venv_exists: bool,
    /// False for a use whose version rant leaves to another file.
    pub wanted: bool,
    pub venv_python: PathBuf,
    /// The interpreter that makes a missing venv.
    pub python: Vec<String>,
}

impl Use {
    pub(super) fn of(lib: &Library, kind: Kind, how: &'static str) -> Use {
        Use {
            library: lib.name.clone(),
            kind,
            how,
            dir: PathBuf::new(),
            file: PathBuf::new(),
            version: None,
            venv_exists: false,
            wanted: true,
            venv_python: PathBuf::new(),
            python: Vec::new(),
        }
    }
}

// ---- CMake ----

pub(super) fn is_fetch(c: &cmake::Call) -> bool {
    c.named(&["CPMAddPackage", "CPMFindPackage", "CPMDeclarePackage", "FetchContent_Declare"])
}

/// A fetch of the library: named after it, or pointing at its repo.
pub(super) fn fetches(text: &str, c: &cmake::Call, lib: &Library) -> bool {
    if !is_fetch(c) {
        return false;
    }
    let args = c.args(text);
    let name = icase(&format!(r"\bNAME\s+{}(?:[^\w-]|$)", regex::escape(&lib.name)));
    let repo = icase(&format!(r#"(?:^|[/:"\s]){}(?:\.git)?(?:[^\w.-]|$)"#, regex::escape(&lib.repo)));
    repo.is_match(args) || cmake::first_arg(args).eq_ignore_ascii_case(&lib.name) || name.is_match(args)
}

/// Where a version sits in a fetch: after GIT_TAG or VERSION, after @ in the CPM short form,
/// in a release download or archive URL.
pub(super) fn version_pattern() -> Regex {
    icase(r#"(GIT_TAG\s+"?|VERSION\s+"?|@|/download/|/archive/(?:refs/tags/)?)(v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z]+)?)"#)
}

pub fn cmake_use(text: &str, lib: &Library) -> Option<Use> {
    let installed = Regex::new(r"^\s*\S+\s+(\d[^\s)]*)").expect("valid");
    for c in cmake::calls(text) {
        let args = c.args(text);
        if fetches(text, &c, lib) {
            let how = if c.name.to_lowercase().starts_with("cpm") { "CPM" } else { "FetchContent" };
            let mut u = Use::of(lib, Kind::CMake, how);
            u.version = version_pattern().captures(args).map(|m| m[3].to_string());
            return Some(u);
        }
        if c.named(&["find_package"]) && cmake::first_arg(args).eq_ignore_ascii_case(&lib.name) {
            let mut u = Use::of(lib, Kind::CMake, "find_package");
            u.version = installed.captures(args).map(|m| m[1].to_string());
            return Some(u);
        }
    }
    None
}

/// The first source a CMakeLists gives target, as written: from add_executable, else
/// target_sources. Variables and generator expressions are passed over.
pub fn cmake_source(text: &str, target: &str) -> Option<String> {
    let source = icase(r"\.(c|cc|cpp|cxx|c\+\+|cu|m|mm)$");
    for c in cmake::calls(text).iter().filter(|c| c.named(&["add_executable", "target_sources"])) {
        let words = cmake::words(c.args(text));
        if words.first() != Some(&target) {
            continue;
        }
        if let Some(w) = words[1..].iter().find(|w| !w.contains("${") && !w.contains("$<") && source.is_match(w)) {
            return Some(w.to_string());
        }
    }
    None
}

/// Whether C# text declares a Main entry point.
pub fn csharp_main(text: &str) -> bool {
    Regex::new(r"\bstatic\s+(async\s+)?(void|int|Task|Task\s*<\s*int\s*>)\s+Main\s*\(").expect("valid").is_match(text)
}

/// The source of a CMake node: its executable's name looked up in every CMakeLists of the package.
fn cmake_node_source(n: &NodeType) -> Option<PathBuf> {
    let stem = n.path.as_deref().and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned());
    let targets: Vec<&str> = stem.as_deref().filter(|s| *s != n.name).into_iter().chain([n.name.as_str()]).collect();
    for dir in discover::folders(&n.cwd).dirs {
        let cmake = dir.join("CMakeLists.txt");
        if !cmake.is_file() {
            continue;
        }
        let text = read(&cmake);
        let found = targets.iter().filter_map(|t| cmake_source(&text, t)).map(|s| dir.join(s)).find(|f| f.is_file());
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The source of a C# node: the file with Main in its project, else Program.cs.
fn csharp_node_source(n: &NodeType) -> Option<PathBuf> {
    let dirs = discover::folders(&n.cwd).dirs;
    let project = dirs.into_iter().find(|d| d.join(format!("{}.csproj", n.name)).is_file()).unwrap_or_else(|| n.cwd.clone());
    for dir in discover::folders(&project).dirs {
        let rel = dir.strip_prefix(&project).unwrap_or(&dir);
        if rel.components().any(|c| ["bin", "obj"].contains(&c.as_os_str().to_string_lossy().to_lowercase().as_str())) {
            continue;
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        files.sort();
        if let Some(f) = files.into_iter().find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("cs")) && csharp_main(&read(p))) {
            return Some(f);
        }
    }
    Some(project.join("Program.cs")).filter(|p| p.is_file())
}

/// The file a node is written in: a Python node's own file, the first source of the CMake target
/// named after it, the C# file holding its Main, a Rust program's source.
pub fn node_source(n: &NodeType) -> Option<PathBuf> {
    match n.kind {
        NodeKind::Python => n.path.clone(),
        NodeKind::Native => cmake_node_source(n),
        NodeKind::CSharp => csharp_node_source(n),
        NodeKind::Rust => n.source.clone(),
        NodeKind::Declared => None,
    }
}

// ---- Python ----

/// A Python project's name in a pattern, as pip compares names: any case, and -, _ and . alike.
fn python_name(project: &str) -> String {
    project.chars().map(|c| if "-_.".contains(c) { "[-_.]+".to_string() } else { regex::escape(&c.to_string()) }).collect()
}

pub(super) fn pin_pattern(lib: &Library) -> Regex {
    let name = python_name(lib.names.python.as_deref().unwrap_or_default());
    icase(&format!(r#"(["']\s*{name}\s*(?:\[[^\]]*\])?\s*(?:===|==|~=|>=|<=|!=|>|<)\s*)(v?)(\d+(?:\.\d+)*(?:[-.]?[0-9A-Za-z]+)*)"#))
}

/// The library's Python project as its wheels and their dist-info folders spell it: lower case,
/// each run of -, _ and . as one _.
pub fn dist_name(lib: &Library) -> String {
    let mut out = String::new();
    for c in lib.names.python.as_deref().unwrap_or_default().to_lowercase().chars() {
        if !"-_.".contains(c) {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    out
}

/// The version of the library installed in a venv, from the dist-info folder its wheel left.
fn venv_version(venv: &Path, lib: &Library) -> Option<String> {
    let lib_dirs = std::fs::read_dir(venv.join("lib")).into_iter().flatten().flatten().map(|e| e.path().join("site-packages"));
    let prefix = format!("{}-", dist_name(lib));
    for site in std::iter::once(venv.join("Lib").join("site-packages")).chain(lib_dirs) {
        for e in std::fs::read_dir(&site).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if let Some(v) = name.strip_prefix(&prefix).and_then(|r| r.strip_suffix(".dist-info")).filter(|v| !v.is_empty()) {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn has_python(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == "py"))
}

/// The pyproject pin and venv of a folder for each library it lists or holds. Python that imports
/// rant is a node, so it counts for the default library, and adding counts any Python.
fn python_uses(dir: &Path, libs: &[&Library], adding: bool, out: &mut Vec<Use>) {
    if libs.is_empty() {
        return;
    }
    let pyproject = dir.join("pyproject.toml");
    let text = read(&pyproject);
    let py = Folder::of(dir);
    for &lib in libs {
        let name = python_name(lib.names.python.as_deref().unwrap_or_default());
        let listed = icase(&format!(r#"["']\s*{name}(?:[^\w.-]|$)"#)).is_match(&text);
        if listed {
            let mut u = Use::of(lib, Kind::Python, "pyproject");
            (u.dir, u.file) = (dir.to_path_buf(), pyproject.clone());
            u.version = pin_pattern(lib).captures(&text).map(|m| m[3].to_string());
            out.push(u);
        }
        let held = py.venv.as_deref().and_then(|v| venv_version(v, lib));
        let node = lib.name == DEFAULT_LIBRARY && py.imports_rant;
        if !(listed || held.is_some() || node || (adding && (pyproject.exists() || has_python(dir)))) {
            continue;
        }
        let mut u = Use::of(lib, Kind::Python, "venv");
        u.dir = dir.to_path_buf();
        match &py.venv {
            Some(venv) => {
                (u.file, u.venv_exists, u.venv_python, u.version) = (venv.clone(), true, python::venv_python(venv), held);
            }
            None => {
                // a new venv goes beside the pyproject, else at the workspace root, else here
                let base = if pyproject.exists() { dir } else { py.root.as_deref().unwrap_or(dir) };
                u.file = base.join(".venv");
                u.venv_python = python::venv_python(&u.file);
                u.python = py.interpreter.clone();
            }
        }
        out.push(u);
    }
}

// ---- C# ----

pub(super) fn reference_pattern(lib: &Library) -> Regex {
    let id = regex::escape(lib.names.csharp.as_deref().unwrap_or_default());
    icase(&format!(r#"<PackageReference\b[^>]*?\bInclude\s*=\s*"{id}"[^>]*?(?:/>|>[\s\S]*?</PackageReference\s*>)"#))
}

/// A NuGet version as an attribute or a child element, whole: 0.0.16, [0.0.16] or a range.
pub(super) fn element_version() -> Regex {
    icase(r#"(\bVersion\s*=\s*"|<Version>\s*)([^"<]*?)(\s*(?:"|</Version>))"#)
}

/// The lowest version a NuGet version or range allows: [0.0.16,) is 0.0.16.
fn nuget_version(v: &str) -> Option<String> {
    let v = v.trim_start_matches(['[', '(', ' ', '\t']);
    let v = v.split(',').next().unwrap_or_default().trim_end_matches([']', ')']).trim_end();
    (!v.is_empty()).then(|| v.to_string())
}

pub fn csharp_use(text: &str, lib: &Library) -> Option<Use> {
    if let Some(m) = reference_pattern(lib).find(text) {
        let mut u = Use::of(lib, Kind::CSharp, "PackageReference");
        u.version = element_version().captures(m.as_str()).and_then(|v| nuget_version(&v[2]));
        return Some(u);
    }
    let id = regex::escape(lib.names.csharp.as_deref().unwrap_or_default());
    let project = icase(&format!(r#"<ProjectReference\s+Include\s*=\s*"[^"]*[/\\]{id}\.csproj""#));
    project.is_match(text).then(|| Use::of(lib, Kind::CSharp, "ProjectReference"))
}

// ---- Rust ----

/// [dependencies], [workspace.dependencies] or [target.'cfg(unix)'.dependencies]
fn dependency_table(section: &str) -> bool {
    section == "dependencies" || section.ends_with(".dependencies")
}

/// The library's dependency in a Cargo.toml as the span of text it takes: a `<crate> = ...`
/// line, or a [dependencies.<crate>] table, also renamed with `package`.
pub(super) fn cargo_entry(text: &str, lib: &Library) -> Option<(usize, usize)> {
    let krate = lib.names.rust.as_deref().unwrap_or_default();
    let renamed = icase(&format!(r#"\bpackage\s*=\s*"{}""#, regex::escape(krate)));
    let key = icase(r#"^\s*"?([A-Za-z0-9_-]+)"?\s*="#);
    let mut section = String::new();
    // the start of an open [*.dependencies.<name>] table, and its name
    let mut table: Option<(usize, String)> = None;
    let mut at = 0;
    loop {
        let end = cmake::line_end(text, at);
        let line = &text[at..end];
        let trimmed = line.trim_start_matches([' ', '\t']);
        let header = trimmed.starts_with('[');
        if header
            && let Some((start, name)) = table.take()
            && (name == krate || renamed.is_match(&text[start..at]))
        {
            return Some((start, at));
        }
        if header {
            let inner = &trimmed[1..];
            section = inner[..inner.find(']').unwrap_or(inner.len())].to_string();
            if let Some(dot) = section.rfind('.')
                && dependency_table(&section[..dot])
            {
                table = Some((at, section[dot + 1..].to_string()));
            }
        } else if table.is_none() && dependency_table(&section) && key.captures(line).is_some_and(|m| &m[1] == krate || renamed.is_match(line)) {
            return Some((at, end));
        }
        if end == text.len() {
            break;
        }
        at = end + 1;
    }
    table.filter(|(start, name)| name == krate || renamed.is_match(&text[*start..])).map(|(start, _)| (start, text.len()))
}

/// Where a version sits in a Cargo entry: a git tag, crate-v as rant writes it or a plain v, a
/// version key, or the entry's own string.
pub(super) fn cargo_versions(lib: &Library) -> [Regex; 3] {
    let krate = regex::escape(lib.names.rust.as_deref().unwrap_or_default());
    [
        icase(r#"(\btag\s*=\s*")((?:crate-)?v?)(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)"#),
        icase(r#"(\bversion\s*=\s*"[=^~]?)()(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)"#),
        icase(&format!(r#"(^\s*"?{krate}"?\s*=\s*"[=^~]?)()(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)"#)),
    ]
}

pub fn cargo_use(text: &str, lib: &Library) -> Option<Use> {
    let (start, end) = cargo_entry(text, lib)?;
    let body = &text[start..end];
    // a workspace or path dependency takes its version from another file
    for (pattern, how) in [(r"\bworkspace\s*=\s*true\b", "Cargo workspace"), (r"\bpath\s*=", "Cargo path")] {
        if icase(pattern).is_match(body) {
            return Some(Use { wanted: false, ..Use::of(lib, Kind::Rust, how) });
        }
    }
    let mut u = Use::of(lib, Kind::Rust, "Cargo");
    u.version = cargo_versions(lib).iter().find_map(|re| re.captures(body)).map(|m| m[3].to_string());
    Some(u)
}

// ---- finding ----

/// The uses of each library in one folder. adding counts any Python file, so a library can be
/// added to it, else Python counts that lists it, holds it, or imports rant for the default.
pub fn in_folder(dir: &Path, libs: &[Library], adding: bool) -> Vec<Use> {
    uses_in(dir, libs, adding, &LazyCell::new(Book::load as fn() -> Book))
}

/// in_folder with the sketchbook read at most once, however many folders share it.
fn uses_in(dir: &Path, libs: &[Library], adding: bool, book: &LazyCell<Book>) -> Vec<Use> {
    let mut out = Vec::new();
    let mut found = |u: Option<Use>, file: &Path| {
        if let Some(mut u) = u {
            (u.dir, u.file) = (dir.to_path_buf(), file.to_path_buf());
            out.push(u);
        }
    };
    let (cmake, cargo, ini) = (dir.join("CMakeLists.txt"), dir.join("Cargo.toml"), dir.join("platformio.ini"));
    let mut projects: Vec<PathBuf> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("csproj"))).collect();
    projects.sort();
    for lib in libs {
        if lib.supports(Kind::CMake) && cmake.is_file() {
            found(cmake_use(&read(&cmake), lib), &cmake);
        }
        if lib.supports(Kind::CSharp) {
            projects.iter().for_each(|p| found(csharp_use(&read(p), lib), p));
        }
        if lib.supports(Kind::Rust) && cargo.is_file() {
            found(cargo_use(&read(&cargo), lib), &cargo);
        }
        if ini.is_file() {
            found(arduino::pio_use(&read(&ini), lib), &ini);
        }
    }
    if arduino::is_sketch(dir) {
        out.extend(libs.iter().filter_map(|lib| book.use_in(dir, lib)));
    }
    let python: Vec<&Library> = libs.iter().filter(|l| l.supports(Kind::Python)).collect();
    python_uses(dir, &python, adding, &mut out);
    out
}

/// The uses in every folder under start, by folder, the walk following the workspace's ignore
/// globs, then the sketchbook's. A venv or sketchbook copy is listed once, under the folder holding it.
pub fn under(start: &Path, libs: &[Library]) -> Vec<Use> {
    let walk = discover::folders(start);
    let mut out: Vec<Use> = Vec::new();
    let book = LazyCell::new(Book::load as fn() -> Book);
    let listed = |out: &[Use], held: &Path, lib: &str| out.iter().any(|o| (o.kind == Kind::Arduino || o.how == "venv") && o.file == held && o.library == lib);
    for d in &walk.dirs {
        for mut u in uses_in(d, libs, false, &book) {
            if u.kind == Kind::Arduino || (u.how == "venv" && u.venv_exists) {
                if listed(&out, &u.file, &u.library) {
                    continue;
                }
                u.dir = u.file.parent().map(Path::to_path_buf).unwrap_or_default();
            }
            out.push(u);
        }
    }
    for venv in &walk.venvs {
        for lib in libs.iter().filter(|l| l.supports(Kind::Python)) {
            if listed(&out, venv, &lib.name) {
                continue;
            }
            let Some(held) = venv_version(venv, lib) else { continue };
            let mut u = Use::of(lib, Kind::Python, "venv");
            u.dir = venv.parent().map(Path::to_path_buf).unwrap_or_default();
            (u.file, u.venv_exists, u.version, u.venv_python) = (venv.clone(), true, Some(held), python::venv_python(venv));
            out.push(u);
        }
    }
    for lib in libs {
        if let Some(u) = book.use_in(book.root(), lib).filter(|u| !listed(&out, &u.file, &lib.name)) {
            out.push(u);
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use rant_config::package;

    fn lib(name: &str, repo: &str, cmake: &str, python: &str, csharp: &str, rust: &str) -> Library {
        let names = package::Library { cmake: Some(cmake.into()), python: Some(python.into()), csharp: Some(csharp.into()), rust: Some(rust.into()) };
        Library { name: name.into(), repo: repo.into(), names }
    }

    pub fn rant() -> Library {
        lib("rant", "KosmosisDire/Rant", "rant::rant_host", "rant-middleware", "Rant", "rant-middleware")
    }

    /// Names that start as Rant's do, so a match must end where the name does.
    pub fn extra() -> Library {
        lib("extra", "KosmosisDire/Rant-extra", "extra::extra", "rant-middleware-extra", "Rant.Extra", "rant-middleware-extra")
    }

    pub const CPM: &str = "cmake_minimum_required(VERSION 3.21)\nproject(cam CXX)\ninclude(cmake/get_cpm.cmake)\n# CPMAddPackage(NAME rant GIT_TAG v0.0.1)\n\
        CPMAddPackage(\n  NAME rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git\n  GIT_TAG \"v0.0.16\" # pinned\n)\nadd_executable(cam main.cpp)\n";

    fn version(u: Option<Use>) -> (Option<&'static str>, Option<String>) {
        u.map_or((None, None), |u| (Some(u.how), u.version))
    }

    #[test]
    fn cmake_fetches_and_installs_are_read() {
        assert_eq!(version(cmake_use(CPM, &rant())), (Some("CPM"), Some("0.0.16".into())));
        assert_eq!(version(cmake_use("CPMAddPackage(\"gh:KosmosisDire/Rant@0.0.16\")\n", &rant())), (Some("CPM"), Some("0.0.16".into())));
        let url = "FetchContent_Declare(rant URL https://github.com/KosmosisDire/Rant/archive/refs/tags/v0.0.16.zip)\n";
        assert_eq!(version(cmake_use(url, &rant())), (Some("FetchContent"), Some("0.0.16".into())));
        assert_eq!(version(cmake_use("find_package(rant 0.0.16 CONFIG REQUIRED)\n", &rant())), (Some("find_package"), Some("0.0.16".into())));
        let other = "CPMAddPackage(NAME extra GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git GIT_TAG v1.2.0)\n";
        assert_eq!(cmake_use(other, &rant()), None);
        assert_eq!(version(cmake_use(other, &extra())), (Some("CPM"), Some("1.2.0".into())));
    }

    #[test]
    fn csharp_references_are_read() {
        let text = "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Newtonsoft.Json\" Version=\"13.0.1\" />\n    \
                    <PackageReference Include=\"Rant\" Version=\"0.0.16\" />\n  </ItemGroup>\n</Project>\n";
        assert_eq!(version(csharp_use(text, &rant())), (Some("PackageReference"), Some("0.0.16".into())));
        let child = "<PackageReference Include=\"Rant\">\n  <Version> [0.0.16,) </Version>\n</PackageReference>";
        assert_eq!(version(csharp_use(child, &rant())).1, Some("0.0.16".into()));
        let cs = "<PackageReference Include=\"Rant.Extra\" Version=\"1.2.0\" />";
        assert_eq!(csharp_use(cs, &rant()), None);
        assert_eq!(version(csharp_use(cs, &extra())).1, Some("1.2.0".into()));
        let project = "<ProjectReference Include=\"..\\rant\\Rant.csproj\" />";
        assert_eq!(version(csharp_use(project, &rant())), (Some("ProjectReference"), None));
    }

    #[test]
    fn cargo_dependencies_are_read_in_every_form() {
        let git = "[package]\nname = \"arm\"\n\n[dependencies]\nserde = \"1\"\nrant-middleware = { git = \"https://github.com/KosmosisDire/Rant.git\", tag = \"crate-v0.0.24\" }\n";
        assert_eq!(version(cargo_use(git, &rant())), (Some("Cargo"), Some("0.0.24".into())));
        let plain = "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = \"=0.0.24\"\nother = \"0.0.25\"\n";
        assert_eq!(version(cargo_use(plain, &rant())).1, Some("0.0.24".into()));
        let renamed = "[package]\nname = \"arm\"\n[dependencies]\nrant = { package = \"rant-middleware\", version = \"0.0.24\" }\n";
        assert_eq!(version(cargo_use(renamed, &rant())).1, Some("0.0.24".into()));
        let table = "[package]\nname = \"arm\"\n[dependencies.rant-middleware]\ngit = \"x\"\ntag = \"v0.0.24\"\n\n[features]\ntag = \"v9.9.9\"\n";
        assert_eq!(version(cargo_use(table, &rant())).1, Some("0.0.24".into()));
        let root = "[workspace]\nmembers = [\"arm\"]\n[workspace.dependencies]\nrant-middleware = { git = \"x\", tag = \"v0.0.24\" }\n";
        assert_eq!(version(cargo_use(root, &rant())).1, Some("0.0.24".into()));
        let member = cargo_use("[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { workspace = true }\n", &rant()).unwrap();
        assert!(member.how == "Cargo workspace" && !member.wanted);
        assert_eq!(cargo_use("[package]\nname = \"a\"\n[dev-dependencies]\nrant-middleware = \"0.0.24\"\n", &rant()), None);
        let extra_dep = "[package]\nname = \"a\"\n[dependencies]\nrant-middleware-extra = { git = \"x\", tag = \"crate-v1.2.0\" }\n";
        assert_eq!(cargo_use(extra_dep, &rant()), None);
        assert_eq!(version(cargo_use(extra_dep, &extra())).1, Some("1.2.0".into()));
    }

    #[test]
    fn a_pyproject_pin_is_read_for_its_own_library() {
        let py = "dependencies = [\"rant-middleware-extra==1.2.0\", \"rant-middleware>=0.0.16\"]\n";
        assert_eq!(pin_pattern(&rant()).captures(py).map(|m| m[3].to_string()), Some("0.0.16".into()));
        assert_eq!(pin_pattern(&extra()).captures(py).map(|m| m[3].to_string()), Some("1.2.0".into()));
        assert_eq!(dist_name(&extra()), "rant_middleware_extra");
    }

    #[test]
    fn cmake_source_is_the_first_source_of_the_target() {
        let text = "add_executable(cam WIN32 \"src/cam main.cpp\" src/cam.h)  # cam\n\
                    add_executable(lidar ${COMMON} $<$<CONFIG:Debug>:debug.cpp> src/lidar.cc)\n\
                    add_library(util src/util.cpp)\nadd_executable(arm)\ntarget_sources(arm PRIVATE src/arm.cpp)\n";
        assert_eq!(cmake_source(text, "cam").as_deref(), Some("src/cam main.cpp"));
        assert_eq!(cmake_source(text, "lidar").as_deref(), Some("src/lidar.cc"));
        assert_eq!(cmake_source(text, "arm").as_deref(), Some("src/arm.cpp"));
        assert_eq!(cmake_source(text, "util"), None);
        assert_eq!(cmake_source("# add_executable(cam main.cpp)\n", "cam"), None);
        assert_eq!(cmake::calls("set(x [==[ ) ]==] y)\nfoo(").len(), 2, "a bracket argument and an unclosed call");
    }

    #[test]
    fn csharp_main_is_found_in_its_forms() {
        assert!(csharp_main("class P { static void Main(string[] a) {} }"));
        assert!(csharp_main("public static async Task<int> Main() => 0;"));
        assert!(!csharp_main("var node = new Rant.Node(\"panel\");"));
    }

    #[test]
    fn a_walk_lists_each_use_by_folder() {
        let t = Scratch::new("walk");
        t.write("cam/CMakeLists.txt", CPM);
        t.write("py/node.py", "import rant\n");
        t.write("py/.venv/pyvenv.cfg", "home = x\n");
        t.write("py/.venv/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
        t.write("other/readme.md", "x");
        let uses = under(&t.0, &[rant()]);
        assert_eq!(uses.iter().map(|u| (u.how, u.version.clone())).collect::<Vec<_>>(), [("CPM", Some("0.0.16".into())), ("venv", Some("0.0.17".into()))]);
        assert_eq!(uses[1].dir, t.0.join("py"));
    }

    #[test]
    fn a_walk_lists_each_library_a_venv_holds() {
        let t = Scratch::new("held");
        t.write("py/node.py", "import rant\n");
        t.write("py/.venv/pyvenv.cfg", "home = x\n");
        t.write("py/.venv/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
        t.write("py/.venv/Lib/site-packages/Rant_Middleware_Extra-1.2.0.dist-info/METADATA", "x");
        t.write("tool/tool.py", "print(1)\n");
        let uses = under(&t.0, &[rant(), extra()]);
        assert_eq!(uses.iter().map(|u| (u.library.as_str(), u.version.clone())).collect::<Vec<_>>(), [("rant", Some("0.0.17".into())), ("extra", Some("1.2.0".into()))]);
        assert!(in_folder(&t.0.join("tool"), &[rant(), extra()], false).is_empty());
        assert_eq!(in_folder(&t.0.join("tool"), &[extra()], true).len(), 1);
    }

    #[test]
    fn a_shared_venv_is_listed_once_where_it_is() {
        let t = Scratch::new("shared");
        t.write("workspace.rant", "");
        t.write(".venv/pyvenv.cfg", "home = x\n");
        t.write("io/ft/ft.py", "import rant\n");
        t.write("services/pick/main.py", "from rant import Node\n");
        let uses = under(&t.0, &[rant()]);
        assert_eq!(uses.len(), 1);
        assert!(rant_config::paths::same(&uses[0].dir, &t.0));
    }

    #[test]
    fn every_venv_holding_a_library_is_listed_by_the_workspace_ignore_rules() {
        let t = Scratch::new("ignore");
        t.write("workspace.rant", "ignore = [\"old/**\"]\n");
        t.write(".gitignore", ".venv/\ntools/\n");
        t.write(".venv/pyvenv.cfg", "home = x\n");
        t.write(".venv/Lib/site-packages/rant_middleware-0.0.18.dist-info/METADATA", "x");
        t.write("tools/env/pyvenv.cfg", "home = x\n");
        t.write("tools/env/Lib/site-packages/rant_middleware-0.0.17.dist-info/METADATA", "x");
        t.write("tools/bare/pyvenv.cfg", "home = x\n");
        t.write("old/env/pyvenv.cfg", "home = x\n");
        t.write("old/env/Lib/site-packages/rant_middleware-0.0.16.dist-info/METADATA", "x");
        let uses = under(&t.0, &[rant()]);
        assert_eq!(uses.iter().map(|u| u.version.clone()).collect::<Vec<_>>(), [Some("0.0.18".into()), Some("0.0.17".into())]);
        assert!(uses[0].wanted && rant_config::paths::same(&uses[1].file, &t.0.join("tools/env")));
    }
}
