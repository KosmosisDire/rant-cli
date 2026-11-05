//! Whole workspace tests: discovery, naming, scanning and references working together.

mod common;

use common::{TestDir, load, load_at, load_with_home};
use rant_config::scan::NodeKind;
use rant_config::{Settings, Workspace, paths};

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
    t.write("py/sub/package.rant", "name = \"inner\"\ndescription = \"Tools\"\nnode \"tool\" {\n  run = \"tool --fast\"\n}\n");
    t.write("py/sub/inner_node.py", PY_NODE);
    t.write("cpp/CMakeLists.txt", "project(lidar_driver)\nfind_package(rant REQUIRED)\n");
    t.write("out/CMakeLists.txt", "find_package(rant)\n");
    t.write("skip/CMakeLists.txt", "find_package(rant)\n");
    t.write("cpp/build/CMakeCache.txt", "");
    t.write("cpp/build/_deps/rant-src/CMakeLists.txt", "add_library(rant::rant ALIAS rant)\n");
    t.write("vendor/sdk/package.rant", "");
    t
}

#[test]
fn finds_exactly_the_packages() {
    let t = mixed();
    let ws = load(&t);
    let mut names: Vec<&str> = ws.model.packages.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["detector", "inner", "lidar_driver", "sdk"]);
}

#[test]
fn a_package_says_what_makes_it_one() {
    let t = mixed();
    t.write("py/sub/pyproject.toml", "[project]\nname = \"inner\"\n");
    let ws = load(&t);
    let package = |name: &str| ws.model.package(name).unwrap();
    assert_eq!(package("detector").kinds(), ["python"]);
    assert_eq!(package("inner").kinds(), ["python", "package.rant"]);
    assert_eq!(package("inner").files(), [t.path("py/sub/pyproject.toml"), t.path("py/sub/package.rant")]);
    assert_eq!(package("inner").description(), Some("Tools"));
    assert_eq!(package("lidar_driver").description(), None);
}

#[test]
fn node_types_stay_in_their_package() {
    let t = mixed();
    let ws = load(&t);
    let detector = ws.model.package("detector").unwrap();
    assert_eq!(detector.nodes.len(), 1, "{:?}", detector.nodes);
    let n = &detector.nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("detector", NodeKind::Python));
    assert!(n.run[0].contains(".venv"), "{:?}", n.run);
    assert!(n.run[1].ends_with("py/detector.py"));

    let inner = ws.model.package("inner").unwrap();
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
    let home = t.path(".home");
    let (_, diags) = Workspace::load(Settings::find(t.root(), &home).unwrap(), t.root(), &home);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("also used by"));
}

#[test]
fn references_in_every_form() {
    let t = mixed();
    let ws = load(&t);
    assert_eq!(ws.resolve("inner/tool").unwrap().name, "tool");
    assert_eq!(ws.resolve("inner_node").unwrap().package, "inner");
    assert_eq!(ws.resolve(" inner_node ").unwrap().package, "inner");
    assert_eq!(ws.resolve("./py/sub:tool").unwrap().package, "inner");
    assert!(ws.resolve("nope/tool").unwrap_err().contains("no package"));
    assert!(ws.resolve("inner/nope").unwrap_err().contains("it has: inner_node, tool"));
    assert!(ws.resolve("./missing").is_err());
    let py = load_at(&t, "py");
    assert_eq!(py.resolve("./detector.py").unwrap().package, "detector");
    let loose = py.resolve("./helper.py").unwrap();
    assert_eq!((loose.name.as_str(), loose.kind), ("helper", NodeKind::Python));
}

#[test]
fn a_csharp_program_runs_its_newest_build() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write(
        "hmi/hmi.csproj",
        "<Project><PropertyGroup><OutputType>Exe</OutputType><AssemblyName>panel</AssemblyName></PropertyGroup><ItemGroup><PackageReference Include=\"Rant\" /></ItemGroup></Project>\n",
    );
    let ws = load(&t);
    let n = &ws.model.package("panel").unwrap().nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("panel", NodeKind::CSharp));
    assert!(n.run.is_empty(), "nothing to run before a build");
    assert!(ws.resolve("panel").is_ok());
    assert!(ws.plan_node("panel").unwrap_err().message.contains("not built yet"));

    t.write("hmi/bin/Debug/net8.0/panel.dll", "");
    let built = load(&t);
    let n = &built.model.package("panel").unwrap().nodes[0];
    assert_eq!(n.run[0], "dotnet");
    assert!(n.run[1].ends_with("bin/Debug/net8.0/panel.dll"));
}

#[test]
fn a_rust_program_runs_its_newest_build_and_cargo_copies_are_no_nodes() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("robot/Cargo.toml", "[package]\nname = \"robot\"\n[dependencies]\nrant-middleware = \"0.0.24\"\n");
    t.write("robot/src/bin/talker.rs", "fn main() {}\n");
    let ws = load(&t);
    let n = &ws.model.package("robot").unwrap().nodes[0];
    assert_eq!((n.name.as_str(), n.kind), ("talker", NodeKind::Rust));
    assert!(n.run.is_empty(), "nothing to run before a build");
    assert!(n.source.as_ref().unwrap().ends_with("src/bin/talker.rs"));

    let exe = if cfg!(windows) { "talker.exe" } else { "talker" };
    t.write("robot/target/CACHEDIR.TAG", "Signature: 8a477f597d28d172789f06886806bc55\n");
    t.write(&format!("robot/target/debug/{exe}"), "");
    t.write(&format!("robot/target/debug/deps/talker-1a2b3c{}", if cfg!(windows) { ".exe" } else { "" }), "");
    let built = load(&t);
    let p = built.model.package("robot").unwrap();
    assert_eq!(p.nodes.len(), 1, "the hashed copy in deps is no node: {:?}", p.nodes);
    assert!(p.nodes[0].run[0].ends_with(&format!("target/debug/{exe}")));
    let step = &built.build_plan(&[]).unwrap().steps[0];
    assert_eq!(step.commands, vec![vec!["cargo", "build"]]);
    assert_eq!(step.clean, [t.root().join("robot/target")]);
}

#[test]
fn a_cargo_workspace_member_runs_from_the_workspace_target() {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write("Cargo.toml", "[workspace]\nmembers = [\"nodes/*\"]\n");
    t.write("nodes/arm/Cargo.toml", "[package]\nname = \"arm\"\n[dependencies]\nrant-middleware = { workspace = true }\n");
    t.write("nodes/arm/src/main.rs", "fn main() {}\n");
    t.write("target/CACHEDIR.TAG", "");
    t.write(&format!("target/release/{}", if cfg!(windows) { "arm.exe" } else { "arm" }), "");
    let ws = load(&t);
    let names: Vec<&str> = ws.model.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["arm"], "the virtual root is no package");
    assert!(ws.model.package("arm").unwrap().nodes[0].run[0].contains("target/release/arm"));
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
    let ws = load(&t);
    let mut names: Vec<&str> = ws.model.packages.iter().map(|p| p.name.as_str()).collect();
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
    let ws = load(&t);
    assert!(ws.model.packages.is_empty(), "a folder of scripts is no package");
    let names: Vec<&str> = ws.model.loose.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["ft", "pick"], "main.py takes its folder's name");
    let ft = ws.resolve("ft").unwrap();
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

    let ws = load_with_home(&t, "ws", &home);
    let packages: Vec<(&str, bool)> = ws.model.packages.iter().map(|p| (p.name.as_str(), p.installed)).collect();
    assert_eq!(packages, [("tools", false), ("bridge", true)], "the workspace tools hides the installed one");
    let groups: Vec<&str> = ws.model.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, ["bridge/Bridge"]);
    assert_eq!(ws.group("Bridge").unwrap().file.name, "bridge/Bridge");
    let bridge = ws.resolve("bridge").unwrap();
    assert_eq!(bridge.kind, NodeKind::Declared);
    assert!(bridge.run[0].ends_with("packages/bridge/bin/rant-bridge"), "{:?}", bridge.run);
    assert!(paths::same(&bridge.cwd, &t.path("home/packages/bridge")));
    let steps: Vec<String> = ws.build_plan(&[]).unwrap().steps.into_iter().map(|s| s.package).collect();
    assert_eq!(steps, ["tools"], "an installed package came built");
    assert!(ws.build_plan(&["bridge".into()]).unwrap_err().message.contains("`bridge` is installed"));

    t.mkdir("elsewhere");
    let outside = load_with_home(&t, "elsewhere", &home);
    assert!(outside.model.settings.home);
    let packages: Vec<&str> = outside.model.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(packages, ["bridge", "tools"]);
    assert!(outside.model.loose.is_empty());
}
