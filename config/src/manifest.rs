//! The build manifests in a directory: what each says about the package's name, whether it
//! uses Rant, and which other packages it names. Text heuristics, never a full parse.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    CMake,
    Python,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub kind: ManifestKind,
    pub path: PathBuf,
    pub name: Option<String>,
    pub uses_rant: bool,
    /// Names this manifest depends on, each with the line that says so.
    pub depends: Vec<Mention>,
    /// Directories this manifest pulls in by path, each with the line that says so.
    pub subdirs: Vec<(PathBuf, String)>,
}

#[derive(Debug, Clone)]
pub struct Mention {
    pub name: String,
    pub source: String,
}

/// The Python distribution the Rant bindings ship as.
pub const RANT_PYTHON_PACKAGE: &str = "rant-middleware";

pub fn in_dir(dir: &Path) -> Vec<Manifest> {
    let mut out = Vec::new();
    if let Some(m) = cmake(&dir.join("CMakeLists.txt")) {
        out.push(m);
    }
    if let Some(m) = pyproject(&dir.join("pyproject.toml")) {
        out.push(m);
    }
    out
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

/// The file with every `#` comment removed, so a commented out call does not count.
fn without_comments(text: &str) -> String {
    text.lines().map(|l| l.split('#').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

fn cmake(path: &Path) -> Option<Manifest> {
    let text = without_comments(&std::fs::read_to_string(path).ok()?);
    static USES: OnceLock<Regex> = OnceLock::new();
    static PROJECT: OnceLock<Regex> = OnceLock::new();
    static FIND: OnceLock<Regex> = OnceLock::new();
    static SUBDIR: OnceLock<Regex> = OnceLock::new();
    let uses = re(
        &USES,
        r"(?i)\bfind_package\s*\(\s*rant\b|\bfetchcontent_declare\s*\(\s*rant\b|\bcpmaddpackage\s*\([^)]*\bname\s+rant\b|\bpkg_check_modules\s*\([^)]*\brant\b|\brant::rant",
    )
    .is_match(&text);
    let name = re(&PROJECT, r"(?i)\bproject\s*\(\s*([A-Za-z0-9_.+-]+)")
        .captures(&text)
        .map(|c| c[1].to_string());
    let depends = re(&FIND, r"(?i)\bfind_package\s*\(\s*([A-Za-z0-9_.+-]+)")
        .captures_iter(&text)
        .map(|c| Mention { name: c[1].to_string(), source: format!("find_package({})", &c[1]) })
        .collect();
    let dir = path.parent().unwrap_or(Path::new("."));
    let subdirs = re(&SUBDIR, r#"(?i)\badd_subdirectory\s*\(\s*"?([^\s")]+)"#)
        .captures_iter(&text)
        .filter(|c| !c[1].contains('$'))
        .map(|c| (dir.join(&c[1]), format!("add_subdirectory({})", &c[1])))
        .collect();
    Some(Manifest { kind: ManifestKind::CMake, path: path.to_path_buf(), name, uses_rant: uses, depends, subdirs })
}

/// PEP 503: case and runs of `-`, `_` and `.` do not matter in a distribution name.
pub fn python_name(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in name.trim().chars() {
        if matches!(c, '-' | '_' | '.') {
            sep = true;
        } else {
            if sep && !out.is_empty() {
                out.push('-');
            }
            sep = false;
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// The distribution name at the front of a PEP 508 requirement such as `numpy>=2; python_version>"3"`.
fn requirement_name(req: &str) -> Option<String> {
    let end = req.find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(req.len());
    let name = req[..end].trim();
    (!name.is_empty()).then(|| python_name(name))
}

fn pyproject(path: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: toml::Table = toml::from_str(&text).ok()?;
    let project = doc.get("project").and_then(|v| v.as_table());
    let name = project.and_then(|p| p.get("name")).and_then(|v| v.as_str()).map(str::to_string);

    let mut reqs: Vec<String> = Vec::new();
    let strings = |v: Option<&toml::Value>| -> Vec<String> {
        v.and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    reqs.extend(strings(project.and_then(|p| p.get("dependencies"))));
    if let Some(opt) = project.and_then(|p| p.get("optional-dependencies")).and_then(|v| v.as_table()) {
        for v in opt.values() {
            reqs.extend(strings(Some(v)));
        }
    }
    if let Some(groups) = doc.get("dependency-groups").and_then(|v| v.as_table()) {
        for v in groups.values() {
            reqs.extend(strings(Some(v)));
        }
    }
    let poetry = doc.get("tool").and_then(|t| t.get("poetry")).and_then(|p| p.get("dependencies")).and_then(|d| d.as_table());
    if let Some(p) = poetry {
        reqs.extend(p.keys().cloned());
    }

    let depends: Vec<Mention> = reqs
        .iter()
        .filter_map(|r| requirement_name(r).map(|n| Mention { name: n, source: format!("dependency {}", r.trim()) }))
        .collect();
    let uses_rant = depends.iter().any(|m| m.name == RANT_PYTHON_PACKAGE);
    Some(Manifest { kind: ManifestKind::Python, path: path.to_path_buf(), name, uses_rant, depends, subdirs: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;

    #[test]
    fn cmake_finds_rant_and_the_project_name() {
        let t = TestDir::new();
        t.write("a/CMakeLists.txt", "project(lidar_driver CXX)\nfind_package(rant REQUIRED)\n");
        t.write("b/CMakeLists.txt", "project(other)\n# find_package(rant)\n");
        t.write("c/CMakeLists.txt", "CPMAddPackage(NAME rant\n  GITHUB_REPOSITORY x/y)\nfind_package(KinovaSDK)\nadd_subdirectory(../sdk sdk)\n");
        let a = &in_dir(&t.path("a"))[0];
        assert!(a.uses_rant);
        assert_eq!(a.name.as_deref(), Some("lidar_driver"));
        assert!(!in_dir(&t.path("b"))[0].uses_rant);
        let c = &in_dir(&t.path("c"))[0];
        assert!(c.uses_rant);
        assert!(c.depends.iter().any(|m| m.name == "KinovaSDK"));
        assert_eq!(c.subdirs[0].0, t.path("c").join("../sdk"));
    }

    #[test]
    fn pyproject_dependencies() {
        let t = TestDir::new();
        t.write(
            "p/pyproject.toml",
            "[project]\nname = \"detector\"\ndependencies = [\"Rant_Middleware>=0.0.17\", \"numpy\"]\n",
        );
        let m = &in_dir(&t.path("p"))[0];
        assert!(m.uses_rant);
        assert_eq!(m.name.as_deref(), Some("detector"));
        assert!(m.depends.iter().any(|d| d.name == "numpy"));
    }

    #[test]
    fn python_names_normalize() {
        assert_eq!(python_name("Rant__Middleware"), "rant-middleware");
        assert_eq!(python_name("a.b-c"), "a-b-c");
    }
}
