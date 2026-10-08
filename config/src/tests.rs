//! Whole workspace tests: discovery, naming, scanning and references working together.

use crate::model::Model;
use crate::paths;
use crate::refs;
use crate::scan::NodeKind;
use crate::testdir::TestDir;
use crate::workspace;

const PY_NODE: &str = "import rant\n\nif __name__ == \"__main__\":\n    pass\n";

fn mixed() -> TestDir {
    let t = TestDir::new();
    t.write("rant.hcl", "workspace {\n  ignore = [\"skip/**\"]\n}\n");
    t.write(".gitignore", "out/\n");
    t.write("py/pyproject.toml", "[project]\nname = \"detector\"\ndependencies = [\"rant-middleware\"]\n");
    t.write("py/detector.py", PY_NODE);
    t.write("py/helper.py", "import rant\n");
    t.write("py/.venv/pyvenv.cfg", "home = x\n");
    t.write("py/.venv/lib/site.py", PY_NODE);
    t.write("py/sub/rant.hcl", "package {\n  name = \"inner\"\n  node \"tool\" {\n    run = \"tool --fast\"\n  }\n}\n");
    t.write("py/sub/inner_node.py", PY_NODE);
    t.write("cpp/CMakeLists.txt", "project(lidar_driver)\nfind_package(rant REQUIRED)\n");
    t.write("out/CMakeLists.txt", "find_package(rant)\n");
    t.write("skip/CMakeLists.txt", "find_package(rant)\n");
    t.write("cpp/build/CMakeCache.txt", "");
    t.write("cpp/build/_deps/rant-src/CMakeLists.txt", "add_library(rant::rant ALIAS rant)\n");
    t.write("vendor/sdk/rant.hcl", "package {}\n");
    t
}

fn load(t: &TestDir) -> Model {
    let config = workspace::open(t.root()).unwrap().unwrap();
    let (model, diags) = Model::load(config);
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
    t.write("rant.hcl", "workspace {}\n");
    t.write("a/rant.hcl", "package {\n  name = \"same\"\n}\n");
    t.write("b/rant.hcl", "package {\n  name = \"same\"\n}\n");
    let (_, diags) = Model::load(workspace::open(t.root()).unwrap().unwrap());
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
    t.write("rant.hcl", "workspace {}\n");
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
fn python_nodes_outside_packages_have_no_package() {
    let t = TestDir::new();
    t.write("rant.hcl", "workspace {}\n");
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
