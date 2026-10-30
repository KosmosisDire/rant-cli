//! Whole workspace tests: discovery, naming, scanning and references working together.

use crate::build_plan;
use crate::group::Groups;
use crate::model::Model;
use crate::paths;
use crate::refs;
use crate::scan::NodeKind;
use crate::testdir::TestDir;
use crate::workspace;

const PY_NODE: &str = "import rant\n\nif __name__ == \"__main__\":\n    pass\n";

fn mixed() -> TestDir {
    let t = TestDir::new();
    t.write("workspace.rant", "ignore = [\"skip/**\"]\n");
    t.write(".gitignore", "out/\n");
    t.write("py/pyproject.toml", "[project]\nname = \"detector\"\ndependencies = [\"rant-middleware\"]\n");
    t.write("py/detector.py", PY_NODE);
    t.write("py/helper.py", "import rant\n");
    t.write("py/.venv/pyvenv.cfg", "home = x\n");
    t.write("py/.venv/lib/site.py", PY_NODE);
    t.write("py/sub/package.rant", "name = \"inner\"\nnode \"tool\" {\n  run = \"tool --fast\"\n}\n");
    t.write("py/sub/inner_node.py", PY_NODE);
    t.write("cpp/CMakeLists.txt", "project(lidar_driver)\nfind_package(rant REQUIRED)\n");
    t.write("out/CMakeLists.txt", "find_package(rant)\n");
    t.write("skip/CMakeLists.txt", "find_package(rant)\n");
    t.write("cpp/build/CMakeCache.txt", "");
    t.write("cpp/build/_deps/rant-src/CMakeLists.txt", "add_library(rant::rant ALIAS rant)\n");
    t.write("vendor/sdk/package.rant", "");
    t
}

fn load(t: &TestDir) -> Model {
    let config = workspace::open(t.root()).unwrap().unwrap();
    let (model, diags) = Model::load(config, None);
    assert!(diags.is_empty(), "{diags:?}");
    model
}

#[test]
fn finds_exactly_the_packages() {
    let t = mixed();
    let m = load(&t);
    let mut names: Vec<&str> = m.packages.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["detector", "inner", "lidar_driver", "sdk"]);
}

#[test]
fn node_types_stay_in_their_package() {
    let t = mixed();
    let m = load(&t);
    let detector = m.package("detector").unwrap();
    assert_eq!(detector.nodes.len(), 1, "{:?}", detector.nodes);
    let n = &detector.nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("detector", NodeKind::Python));
    assert!(n.run[0].contains(".venv"), "{:?}", n.run);
    assert!(n.run[1].ends_with("py/detector.py"));

    let inner = m.package("inner").unwrap();
    let names: Vec<&str> = inner.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["inner_node", "tool"]);
    assert_eq!(inner.nodes[1].run, ["tool", "--fast"]);
    assert_eq!(inner.nodes[1].kind, NodeKind::Declared);
}

#[test]
fn duplicate_package_names_are_an_error() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("a/package.rant", "name = \"same\"\n");
    t.write("b/package.rant", "name = \"same\"\n");
    let (_, diags) = Model::load(workspace::open(t.root()).unwrap().unwrap(), None);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("also used by"));
}

#[test]
fn references_in_every_form() {
    let t = mixed();
    let m = load(&t);
    assert_eq!(refs::resolve(&m, "inner/tool", t.root()).unwrap().name, "tool");
    assert_eq!(refs::resolve(&m, "inner_node", t.root()).unwrap().package, "inner");
    assert_eq!(refs::resolve(&m, " inner_node ", t.root()).unwrap().package, "inner");
    assert_eq!(refs::resolve(&m, "./py/sub:tool", t.root()).unwrap().package, "inner");
    let by_path = refs::resolve(&m, "./detector.py", &t.path("py")).unwrap();
    assert_eq!(by_path.package, "detector");
    let loose = refs::resolve(&m, "./helper.py", &t.path("py")).unwrap();
    assert_eq!((loose.name.as_str(), loose.kind), ("helper", NodeKind::Python));
    assert!(refs::resolve(&m, "nope/tool", t.root()).unwrap_err().contains("no package"));
    assert!(refs::resolve(&m, "inner/nope", t.root()).unwrap_err().contains("it has: inner_node, tool"));
    assert!(refs::resolve(&m, "./missing", t.root()).is_err());
}

#[test]
fn a_csharp_program_runs_its_newest_build() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write(
        "hmi/hmi.csproj",
        "<Project><PropertyGroup><OutputType>Exe</OutputType><AssemblyName>panel</AssemblyName></PropertyGroup><ItemGroup><PackageReference Include=\"Rant\" /></ItemGroup></Project>\n",
    );
    let m = load(&t);
    let n = &m.package("panel").unwrap().nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("panel", NodeKind::CSharp));
    assert!(n.run.is_empty(), "nothing to run before a build");
    assert!(refs::resolve(&m, "panel", t.root()).is_ok());
    assert!(crate::plan::single(&m, "panel", t.root()).unwrap_err().message.contains("not built yet"));

    t.write("hmi/bin/Debug/net8.0/panel.dll", "");
    let built = load(&t);
    let n = &built.package("panel").unwrap().nodes[0];
    assert_eq!(n.run[0], "dotnet");
    assert!(n.run[1].ends_with("bin/Debug/net8.0/panel.dll"));
}

#[test]
fn a_rust_program_runs_its_newest_build_and_cargo_copies_are_no_nodes() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("robot/Cargo.toml", "[package]
name = \"robot\"
[dependencies]
rant-middleware = \"0.0.24\"
");
    t.write("robot/src/bin/talker.rs", "fn main() {}
");
    let m = load(&t);
    let n = &m.package("robot").unwrap().nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("talker", NodeKind::Rust));
    assert!(n.run.is_empty(), "nothing to run before a build");
    assert!(n.source.as_ref().unwrap().ends_with("src/bin/talker.rs"));

    let exe = if cfg!(windows) { "talker.exe" } else { "talker" };
    t.write("robot/target/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55
");
    t.write(&format!("robot/target/debug/{exe}"), "");
    t.write(&format!("robot/target/debug/deps/talker-1a2b3c{}", if cfg!(windows) { ".exe" } else { "" }), "");
    let built = load(&t);
    let p = built.package("robot").unwrap();
    assert_eq!(p.nodes.len(), 1, "the hashed copy in deps is no node: {:?}", p.nodes);
    assert!(p.nodes[0].run[0].ends_with(&format!("target/debug/{exe}")));
    let step = &crate::build_plan::plan(&built, &[]).unwrap().steps[0];
    assert_eq!(step.commands, vec![vec!["cargo", "build"]]);
    assert_eq!(step.clean, [t.root().join("robot/target")]);
}

#[test]
fn a_cargo_workspace_member_runs_from_the_workspace_target() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("Cargo.toml", "[workspace]
members = [\"nodes/*\"]
");
    t.write("nodes/arm/Cargo.toml", "[package]
name = \"arm\"
[dependencies]
rant-middleware = { workspace = true }
");
    t.write("nodes/arm/src/main.rs", "fn main() {}
");
    t.write("target/CACHEDIR.TAG", "");
    t.write(&format!("target/release/{}", if cfg!(windows) { "arm.exe" } else { "arm" }), "");
    let m = load(&t);
    let names: Vec<&str> = m.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["arm"], "the virtual root is no package");
    assert!(m.package("arm").unwrap().nodes[0].run[0].contains("target/release/arm"));
}

#[test]
fn the_scan_cache_is_written_and_reused() {
    let t = mixed();
    load(&t);
    let cache = t.path(".rant/cache/scan.json");
    assert!(cache.is_file());
    let before = std::fs::metadata(&cache).unwrap().modified().unwrap();
    load(&t);
    assert_eq!(std::fs::metadata(&cache).unwrap().modified().unwrap(), before, "an unchanged tree rewrites nothing");
}

#[test]
fn a_cmake_folder_inside_a_cmake_package_is_part_of_it() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("CMakeLists.txt", "project(arna_system)\nCPMAddPackage(NAME rant GIT_TAG v0.0.18)\nadd_subdirectory(src/ft)\n");
    t.write("src/ft/CMakeLists.txt", "add_executable(ft ft.cpp)\ntarget_link_libraries(ft PRIVATE rant::rant_host)\n");
    t.write("src/arm/CMakeLists.txt", "add_executable(arm arm.cpp)\ntarget_link_libraries(arm PRIVATE rant::rant_host)\n");
    t.write("src/arm/package.rant", "name = \"arm\"\n");
    t.write("tools/py/pyproject.toml", "[project]\nname = \"tool\"\ndependencies = [\"rant-middleware\"]\n");
    let m = load(&t);
    let mut names: Vec<&str> = m.packages.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["arm", "arna_system", "tool"], "a block keeps arm, ft belongs to the top");
}

#[test]
fn python_nodes_outside_packages_have_no_package() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("Services/pick/main.py", "import rant\n\ndef main():\n    pass\n");
    t.write("Services/pick/schemas.py", "import rant\n");
    t.write("IO/ft/ft.py", "import rant\nnode = rant.Node()\n");
    t.write("IO/lector/lector.py", "import serial\n");
    let m = load(&t);
    assert!(m.packages.is_empty(), "a folder of scripts is no package");
    let names: Vec<&str> = m.loose.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["ft", "pick"], "main.py takes its folder's name");
    let ft = refs::resolve(&m, "ft", t.root()).unwrap();
    assert_eq!(ft.package, "");
    assert!(paths::same(&ft.cwd, &t.path("IO/ft")));
}

#[test]
fn installed_packages_join_a_workspace_and_stand_alone_outside_one() {
    let t = TestDir::new();
    t.write("ws/workspace.rant", "");
    t.write("ws/tools/package.rant", "name = \"tools\"\nnode \"helper\" {\n  run = \"helper\"\n}\n");
    t.write("home/packages/bridge/package.rant", "node \"bridge\" {\n  run = \"${path.package}/bin/rant-bridge\"\n}\n");
    t.write("home/packages/bridge/Bridge.run", "node \"bridge\" {}\n");
    t.write("home/packages/tools/package.rant", "node \"old\" {\n  run = \"old\"\n}\n");
    t.write("home/packages/notes/readme.txt", "no package.rant, no package\n");
    let home = t.path("home");

    let (m, diags) = Model::load(workspace::open(&t.path("ws")).unwrap().unwrap(), Some(&home));
    assert!(diags.is_empty(), "{diags:?}");
    let packages: Vec<(&str, bool)> = m.packages.iter().map(|p| (p.name.as_str(), p.installed)).collect();
    assert_eq!(packages, [("tools", false), ("bridge", true)], "the workspace tools hides the installed one");
    let groups: Vec<&str> = m.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, ["bridge/Bridge"]);
    assert_eq!(Groups::new(&m).find("Bridge", None).unwrap().name, "bridge/Bridge");
    let bridge = refs::resolve(&m, "bridge", &t.path("ws")).unwrap();
    assert_eq!(bridge.kind, NodeKind::Declared);
    assert!(bridge.run[0].ends_with("packages/bridge/bin/rant-bridge"), "{:?}", bridge.run);
    assert!(paths::same(&bridge.cwd, &t.path("home/packages/bridge")));
    let steps: Vec<String> = build_plan::plan(&m, &[]).unwrap().steps.into_iter().map(|s| s.package).collect();
    assert_eq!(steps, ["tools"], "an installed package came built");
    assert!(build_plan::plan(&m, &["bridge".into()]).unwrap_err().message.contains("`bridge` is installed"));

    let (m, diags) = Model::load(workspace::home(&home).unwrap(), Some(&home));
    assert!(diags.is_empty(), "{diags:?}");
    let packages: Vec<&str> = m.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(packages, ["bridge", "tools"]);
    assert!(m.loose.is_empty());
}
