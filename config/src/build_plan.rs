//! The build plan: which packages build, in what order, with which commands. rant only
//! orders builds. It never tells a build system where its dependencies are.

use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::manifest::{python_name, ManifestKind};
use crate::model::{Model, Package};
use crate::paths;
use crate::workspace::WORKSPACE_FILE;

/// One package's build: a CMake configure when the build folder needs one, then commands
/// run in order in the package directory.
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub package: String,
    pub dir: PathBuf,
    /// The CMake configure command, none for a package without CMake.
    pub configure: Option<Vec<String>>,
    /// The CMake build folder is not configured yet, so configure runs first.
    pub unconfigured: bool,
    /// The CMake build folder.
    pub build_dir: Option<PathBuf>,
    /// Where the CMake cache was made when that is not the build folder: the package moved
    /// or was renamed, and CMake refuses a cache from elsewhere.
    pub moved_from: Option<PathBuf>,
    /// What a clean build removes first: the CMake build folder, C# bin and obj, a cargo target.
    pub clean: Vec<PathBuf>,
    pub commands: Vec<Vec<String>>,
}

/// `from` depends on `to`, and `source` says where that was found.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub source: String,
}

#[derive(Debug, Clone, Default)]
pub struct BuildPlan {
    pub steps: Vec<Step>,
    pub edges: Vec<Edge>,
}

fn slash(p: &Path) -> String {
    crate::eval::slash(p)
}

/// What building a package runs: its `build` when set, else the default of its manifests.
fn step(pkg: &Package, root: &Path) -> Step {
    let mut s = Step { package: pkg.name.clone(), dir: pkg.dir.clone(), ..Step::default() };
    if let Some(build) = pkg.block.as_ref().and_then(|b| b.build.clone()) {
        s.commands = build;
        return s;
    }
    for m in pkg.manifests.iter().filter(|m| m.kind == ManifestKind::CSharp) {
        s.commands.push(vec!["dotnet".into(), "build".into(), slash(&m.path)]);
        if let Some(dir) = m.path.parent() {
            s.clean.extend([dir.join("bin"), dir.join("obj")]);
        }
    }
    if pkg.manifests.iter().any(|m| m.kind == ManifestKind::CMake) {
        // Commands run in the package directory, so the short relative form is exact. A failed
        // configure leaves CMakeCache.txt, only a finished one writes TargetDirectories.txt.
        let build = pkg.dir.join("build");
        s.configure = Some(vec!["cmake".into(), "-S".into(), ".".into(), "-B".into(), "build".into()]);
        s.unconfigured = !build.join("CMakeFiles/TargetDirectories.txt").is_file();
        s.moved_from = moved_cache(&build);
        s.clean.insert(0, build.clone());
        s.build_dir = Some(build);
        s.commands.push(vec!["cmake".into(), "--build".into(), "build".into()]);
    }
    if pkg.manifests.iter().any(|m| m.kind == ManifestKind::Rust) {
        s.commands.push(vec!["cargo".into(), "build".into()]);
        // a target folder outside the workspace, such as a shared CARGO_TARGET_DIR, is never removed
        let target = crate::model::cargo_target(&pkg.dir, root);
        if paths::within(&target, root) {
            s.clean.push(target);
        }
    }
    s
}

/// The folder a CMake cache was made in, when that is not `build`.
fn moved_cache(build: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(build.join("CMakeCache.txt")).ok()?;
    let made = PathBuf::from(text.lines().find_map(|l| l.strip_prefix("CMAKE_CACHEFILE_DIR:INTERNAL="))?.trim());
    let same = matches!((made.canonicalize(), build.canonicalize()), (Ok(a), Ok(b)) if a == b);
    (!same).then_some(made)
}

fn shown(p: &Path, root: &Path) -> String {
    paths::relative(p, root)
}

/// Every dependency between workspace packages: declared ones, then the ones the manifests
/// imply. A declared name that is no package is an error.
pub fn edges(model: &Model) -> Result<Vec<Edge>, Diag> {
    let root = &model.config.root;
    let mut out: Vec<Edge> = Vec::new();
    let add = |out: &mut Vec<Edge>, from: &Package, to: &str, source: String| {
        if from.name != to && !out.iter().any(|e| e.from == from.name && e.to == to) {
            out.push(Edge { from: from.name.clone(), to: to.to_string(), source });
        }
    };
    for pkg in &model.packages {
        if let Some(b) = &pkg.block {
            for d in &b.depend {
                if model.package(d).is_none() {
                    return Err(Diag::file(&b.file, format!("`depend` names `{d}`, which is not a package here")));
                }
                add(&mut out, pkg, d, format!("depend in {}", shown(&b.file, root)));
            }
        }
        for m in &pkg.manifests {
            let file = shown(&m.path, root);
            for mention in &m.depends {
                let target = match m.kind {
                    ManifestKind::CMake | ManifestKind::CSharp | ManifestKind::Rust => model.package(&mention.name),
                    ManifestKind::Python => model.packages.iter().find(|p| python_name(&p.name) == mention.name),
                };
                if let Some(t) = target {
                    add(&mut out, pkg, &t.name, format!("{} in {file}", mention.source));
                }
            }
            for (dir, source) in &m.subdirs {
                if let Some(t) = model.package_holding(&paths::normalize(dir)) {
                    if t.name != pkg.name {
                        add(&mut out, pkg, &t.name, format!("{source} in {file}"));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The build plan for `wanted` (empty = every package) and all they depend on, in
/// dependency order with ties broken by path.
pub fn plan(model: &Model, wanted: &[String]) -> Result<BuildPlan, Diag> {
    // an installed package came built, so nothing waits for it
    let installed = |name: &str| model.package(name).is_some_and(|p| p.installed);
    let edges: Vec<Edge> = edges(model)?.into_iter().filter(|e| !installed(&e.to)).collect();
    for w in wanted {
        match model.package(w) {
            None => return Err(Diag::plain(format!("no package named `{w}`, see `rant ls packages`"))),
            Some(p) if p.installed => return Err(Diag::plain(format!("`{w}` is installed, it came built"))),
            Some(_) => {}
        }
    }

    let mut selected: Vec<bool> =
        model.packages.iter().map(|p| !p.installed && (wanted.is_empty() || wanted.contains(&p.name))).collect();
    loop {
        let mut grew = false;
        for e in &edges {
            let from = model.packages.iter().position(|p| p.name == e.from).unwrap();
            let to = model.packages.iter().position(|p| p.name == e.to).unwrap();
            if selected[from] && !selected[to] {
                selected[to] = true;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    // Kahn: among the ready packages the first by path goes next. Packages are path sorted.
    let mut done: Vec<bool> = vec![false; model.packages.len()];
    let mut order: Vec<usize> = Vec::new();
    let count = selected.iter().filter(|s| **s).count();
    while order.len() < count {
        let next = (0..model.packages.len()).find(|&i| {
            selected[i]
                && !done[i]
                && edges.iter().filter(|e| e.from == model.packages[i].name).all(|e| {
                    let to = model.packages.iter().position(|p| p.name == e.to).unwrap();
                    done[to]
                })
        });
        match next {
            Some(i) => {
                done[i] = true;
                order.push(i);
            }
            None => return Err(cycle(model, &edges, &selected, &done)),
        }
    }

    let used: Vec<Edge> = edges
        .into_iter()
        .filter(|e| order.iter().any(|&i| model.packages[i].name == e.from))
        .collect();
    Ok(BuildPlan { steps: order.into_iter().map(|i| step(&model.packages[i], &model.config.root)).collect(), edges: used })
}

/// The error for a dependency cycle, naming every edge on it and where each came from.
fn cycle(model: &Model, edges: &[Edge], selected: &[bool], done: &[bool]) -> Diag {
    let left: Vec<&str> =
        (0..model.packages.len()).filter(|&i| selected[i] && !done[i]).map(|i| model.packages[i].name.as_str()).collect();
    let mut path: Vec<&Edge> = Vec::new();
    let mut at = left[0];
    while !path.iter().any(|e| e.from == at) {
        let Some(e) = edges.iter().find(|e| e.from == at && left.contains(&e.to.as_str())) else { break };
        path.push(e);
        at = &e.to;
    }
    let start = path.iter().position(|e| e.from == at).unwrap_or(0);
    let lines: Vec<String> = path[start..].iter().map(|e| format!("  {} -> {} ({})", e.from, e.to, e.source)).collect();
    let file = model.config.root.join(WORKSPACE_FILE);
    Diag::file(&file, format!("the packages depend on each other in a cycle:\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TestDir;
    use crate::workspace;

    fn model(t: &TestDir) -> Model {
        let (m, d) = Model::load(workspace::open(t.root()).unwrap().unwrap(), None);
        assert!(d.is_empty(), "{d:?}");
        m
    }

    fn workspace_tree() -> TestDir {
        let t = TestDir::new();
        t.write("workspace.rant", "");
        t.write("a_sdk/package.rant", "name  = \"sdk\"\nbuild = [\"make\", \"make install\"]\n");
        t.write("b_driver/CMakeLists.txt", "project(driver)\nfind_package(rant)\nfind_package(sdk)\n");
        t.write("c_app/CMakeLists.txt", "project(app)\nfind_package(rant)\nfind_package(driver)\n");
        t.write("b_driver/build/CMakeCache.txt", "");
        t.write("c_app/build/CMakeFiles/TargetDirectories.txt", "");
        t.write("d_py/pyproject.toml", "[project]\nname = \"vision\"\ndependencies = [\"rant-middleware\"]\n");
        t.write("d_py/package.rant", "depend = [\"app\"]\n");
        t
    }

    #[test]
    fn dependencies_come_first_and_each_edge_says_why() {
        let t = workspace_tree();
        let p = plan(&model(&t), &[]).unwrap();
        let order: Vec<&str> = p.steps.iter().map(|s| s.package.as_str()).collect();
        assert_eq!(order, ["sdk", "driver", "app", "vision"]);
        assert!(p.edges.contains(&Edge { from: "driver".into(), to: "sdk".into(), source: "find_package(sdk) in b_driver/CMakeLists.txt".into() }));
        assert!(p.edges.iter().any(|e| e.from == "vision" && e.source == "depend in d_py/package.rant"));
    }

    #[test]
    fn default_and_declared_commands() {
        let t = workspace_tree();
        let p = plan(&model(&t), &[]).unwrap();
        assert_eq!(p.steps[0].commands, vec![vec!["make"], vec!["make", "install"]]);
        assert!(p.steps[1].unconfigured, "a failed configure leaves only the cache");
        assert_eq!(p.steps[1].commands[0][..2], ["cmake", "--build"]);
        assert!(!p.steps[2].unconfigured, "already configured");
        assert!(p.steps[3].commands.is_empty(), "python builds nothing");
        assert!(p.steps[0].clean.is_empty() && p.steps[0].configure.is_none(), "declared commands leave nothing rant knows");
        assert_eq!(p.steps[2].clean, [t.root().join("c_app/build")]);
    }

    #[test]
    fn a_cache_made_in_another_folder_is_found() {
        let t = workspace_tree();
        let here = t.root().join("c_app/build");
        t.write("c_app/build/CMakeCache.txt", &format!("CMAKE_CACHEFILE_DIR:INTERNAL={}\n", crate::eval::slash(&here)));
        t.write("b_driver/build/CMakeCache.txt", "CMAKE_CACHEFILE_DIR:INTERNAL=/home/someone/Driver/build\n");
        let p = plan(&model(&t), &[]).unwrap();
        assert_eq!(p.steps[1].moved_from, Some(PathBuf::from("/home/someone/Driver/build")));
        assert_eq!(p.steps[2].moved_from, None);
    }

    #[test]
    fn a_csharp_project_cleans_its_bin_and_obj() {
        let t = TestDir::new();
        t.write("workspace.rant", "");
        t.write("arm/Arm.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><PackageReference Include=\"Rant\" Version=\"0.0.23\" /></ItemGroup></Project>\n");
        let p = plan(&model(&t), &[]).unwrap();
        assert_eq!(p.steps[0].clean, [t.root().join("arm/bin"), t.root().join("arm/obj")]);
        assert!(p.steps[0].build_dir.is_none());
    }

    #[test]
    fn a_selection_takes_its_dependencies_along() {
        let t = workspace_tree();
        let p = plan(&model(&t), &["app".to_string()]).unwrap();
        let order: Vec<&str> = p.steps.iter().map(|s| s.package.as_str()).collect();
        assert_eq!(order, ["sdk", "driver", "app"]);
        assert!(plan(&model(&t), &["nope".to_string()]).is_err());
    }

    #[test]
    fn a_cycle_names_every_edge() {
        let t = workspace_tree();
        t.write("a_sdk/package.rant", "name   = \"sdk\"\ndepend = [\"app\"]\n");
        let err = plan(&model(&t), &[]).unwrap_err();
        assert!(err.message.contains("cycle"), "{err}");
        assert!(err.message.contains("sdk -> app (depend in a_sdk/package.rant)"), "{err}");
        assert!(err.message.contains("driver -> sdk (find_package(sdk)"), "{err}");
    }

    #[test]
    fn an_unknown_depend_is_an_error() {
        let t = workspace_tree();
        t.write("d_py/package.rant", "depend = [\"ghost\"]\n");
        assert!(plan(&model(&t), &[]).unwrap_err().message.contains("`ghost`"));
    }
}
