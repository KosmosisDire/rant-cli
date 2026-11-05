//! `rant new`: a workspace or package goes in the folder named, here by default, and takes
//! its name. A node or group is a file here, named as asked or after this folder.

use std::path::PathBuf;

use rant_config::paths;
use rant_config::python::Folder;
use rant_process::{Command, find_program, output};

use super::{Added, Made};
use crate::cmake;
use crate::context::Context;
use crate::failure::{Result, failed, usage};
use crate::names::pascal;
use crate::scaffold::{self, Values};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// workspace, package, node or group
    #[arg(value_name = "kind")]
    pub kind: Option<String>,
    /// the folder of a workspace or package, the name of a node or group
    #[arg(value_name = "name")]
    pub name: Option<String>,
    #[arg(hide = true)]
    pub more: Vec<String>,
    /// cpp, python, csharp or rust, for a node the package's own by default
    #[arg(long, value_name = "L")]
    pub lang: Option<String>,
}

const KINDS: [&str; 4] = ["workspace", "package", "node", "group"];
const LANGUAGES: [&str; 4] = ["cpp", "python", "csharp", "rust"];

pub fn run(ctx: &mut Context, args: &Args) -> Result<Made> {
    let Some(kind) = args.kind.as_deref().filter(|k| KINDS.contains(k)) else {
        return Err(usage("say what to make: `rant new workspace`, `rant new package`, `rant new node` or `rant new group`"));
    };
    if !args.more.is_empty() {
        return Err(usage(format!("new {kind} takes one folder or name")));
    }
    let preset = chosen(ctx, kind, args.lang.as_deref())?;
    // a C# node is a project of its own, so it is the package preset
    let folder = kind == "workspace" || kind == "package" || preset == "package-csharp";
    let cwd = ctx.cwd()?.to_path_buf();
    let mut dest = match (&args.name, folder) {
        (Some(n), true) => paths::normalize(&cwd.join(n)),
        _ => cwd,
    };
    let mut name = match (&args.name, folder) {
        (Some(n), false) => n.clone(),
        _ => dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    if kind == "group" {
        name = pascal(&name);
    }
    // a Rust node is a src/bin file of the Cargo package around it, which Cargo finds itself
    if preset == "node-rust" {
        dest = cargo_package(ctx, &name)?;
    }
    let mut values = Values::from([("name", name.clone()), ("Name", pascal(&name))]);
    if preset.contains("csharp") {
        values.insert("framework", dotnet_framework());
    }
    let files = scaffold::plan(&preset, &dest, &values)?;

    // a C++ node joins the CMake project around it, which refuses a second target of its name
    let added = if preset == "node-cpp" {
        let project = cmake_project(ctx, &name)?;
        let source = paths::relative(&files[0].0, project.parent().unwrap_or(&project));
        cmake::add_node(&project, &name, &source)?;
        Some(Added { node: name, to: paths::slash(&project) })
    } else {
        None
    };
    scaffold::write(&files)?;
    Ok(Made { created: files.iter().map(|(p, _)| paths::slash(p)).collect(), added })
}

/// The preset a command line asks for, once the tools of its language are here.
fn chosen(ctx: &mut Context, kind: &str, lang: Option<&str>) -> Result<String> {
    if kind == "group" {
        return Ok("group".into());
    }
    let lang = match lang.filter(|l| !l.is_empty()) {
        Some(l) => Some(l.to_string()),
        None if kind == "node" => language_here(ctx)?.map(String::from),
        None => None,
    };
    let Some(lang) = lang else { return Err(usage("say which language with --lang cpp, python, csharp or rust")) };
    if !LANGUAGES.contains(&lang.as_str()) {
        return Err(usage(format!("unknown language `{lang}`, use cpp, python, csharp or rust")));
    }
    // a venv is made only for a Python package or workspace
    require_tools(ctx, &lang, kind != "node")?;
    Ok(match kind {
        "workspace" => format!("workspace-{lang}"),
        "node" if lang != "csharp" => format!("node-{lang}"),
        _ => format!("package-{lang}"),
    })
}

/// The folders from here up to the workspace root, or to the top outside a workspace.
fn folders_up(ctx: &mut Context) -> Result<Vec<PathBuf>> {
    let root = ctx.workspace().map(|s| s.root.clone());
    let mut out = Vec::new();
    for dir in ctx.cwd()?.ancestors() {
        out.push(dir.to_path_buf());
        if root.as_deref().is_some_and(|r| paths::same(dir, r)) {
            break;
        }
    }
    Ok(out)
}

/// The language of the package around here, by its build files.
fn language_here(ctx: &mut Context) -> Result<Option<&'static str>> {
    for dir in folders_up(ctx)? {
        for (file, lang) in [("CMakeLists.txt", "cpp"), ("pyproject.toml", "python"), ("Cargo.toml", "rust")] {
            if dir.join(file).exists() {
                return Ok(Some(lang));
            }
        }
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            match e.path().extension().and_then(|x| x.to_str()) {
                Some("csproj") => return Ok(Some("csharp")),
                Some("py") => return Ok(Some("python")),
                _ => {}
            }
        }
    }
    Ok(None)
}

/// The CMakeLists.txt a C++ node made here joins: the nearest here or above.
fn cmake_project(ctx: &mut Context, name: &str) -> Result<PathBuf> {
    let found = folders_up(ctx)?.into_iter().map(|d| d.join("CMakeLists.txt")).find(|f| f.is_file());
    found.ok_or_else(|| failed(format!("no CMakeLists.txt here or above to add `{name}` to, make a package with `rant new package --lang cpp`")))
}

/// The Cargo package a Rust node made here joins: the nearest Cargo.toml with a [package].
fn cargo_package(ctx: &mut Context, name: &str) -> Result<PathBuf> {
    let package = |d: &PathBuf| std::fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|t| t.contains("[package]"));
    let found = folders_up(ctx)?.into_iter().find(package);
    found.ok_or_else(|| failed(format!("no Cargo package here or above to add `{name}` to, make one with `rant new package --lang rust`")))
}

/// Refuses a language that cannot build or run here, before any file is written.
fn require_tools(ctx: &mut Context, lang: &str, venv: bool) -> Result<()> {
    let tmp = std::env::temp_dir();
    let have = |program: &str| find_program(program, &tmp).is_some();
    let mut missing: Vec<&str> = Vec::new();
    let (shown, apt) = match lang {
        "cpp" => {
            missing.extend(["cmake", "git"].into_iter().filter(|p| !have(p)));
            if !cfg!(windows) && !["c++", "g++", "clang++"].iter().any(|p| have(p)) {
                missing.push("a C++ compiler");
            }
            ("C++", "build-essential cmake git")
        }
        "python" => {
            let mut python = Folder::of(ctx.cwd()?).interpreter;
            if python.first().is_none_or(|p| !have(p)) {
                missing.push("Python");
            } else if venv {
                python.extend(["-c".into(), "import importlib.util as u, sys; sys.exit(u.find_spec('ensurepip') is None)".into()]);
                if output(&Command::new(python, &tmp)).is_none() {
                    missing.push("its venv module");
                }
            }
            ("Python", "python3 python3-venv")
        }
        "rust" => {
            if !have("cargo") {
                missing.push("cargo (from https://rustup.rs)");
            }
            // the Rant crate compiles its C sources
            if !cfg!(windows) && !["cc", "gcc", "clang"].iter().any(|p| have(p)) {
                missing.push("a C compiler");
            }
            ("Rust", "build-essential")
        }
        _ => {
            if output(&Command::new(["dotnet", "--list-sdks"], &tmp)).is_none_or(|sdks| sdks.trim().is_empty()) {
                missing.push("the .NET SDK");
            }
            ("C#", "dotnet-sdk-10.0")
        }
    };
    if missing.is_empty() {
        return Ok(());
    }
    let mut message = format!("{shown} needs {}, nothing was made", spoken(&missing));
    if cfg!(target_os = "linux") {
        message += &format!(". On Ubuntu: `sudo apt install {apt}`");
    }
    Err(failed(message))
}

/// "a", "a and b", "a, b and c"
fn spoken(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The newest target framework the installed .NET SDKs build, such as net10.0, from lines
/// such as `10.0.300 [path]`.
fn dotnet_framework() -> String {
    let sdks = output(&Command::new(["dotnet", "--list-sdks"], std::env::temp_dir())).unwrap_or_default();
    let major = |line: &str| line.split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse::<u32>().ok());
    format!("net{}.0", sdks.lines().filter_map(major).max().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn lists_read_as_spoken() {
        assert_eq!(spoken(&["cmake"]), "cmake");
        assert_eq!(spoken(&["cmake", "git"]), "cmake and git");
        assert_eq!(spoken(&["cmake", "git", "a C++ compiler"]), "cmake, git and a C++ compiler");
    }

    #[test]
    fn a_relative_source_has_forward_slashes() {
        let base = Path::new("/w/robot");
        assert_eq!(paths::relative(&base.join("nodes").join("arm.cpp"), base), "nodes/arm.cpp");
    }
}
