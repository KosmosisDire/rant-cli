mod common;
use common::*;

const CAM: &str = "cmake_minimum_required(VERSION 3.21)
project(cam CXX)
include(cmake/get_cpm.cmake)
CPMAddPackage(
  NAME rant
  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git
  GIT_TAG v0.0.16)
add_executable(cam main.cpp)
";

#[test]
fn library() {
    let t = Test::new("library", 150);
    let home = &t.rant_home;
    t.cache_database(&[
        RANT_LIBRARY,
        r#"{"name": "extra", "repo": "KosmosisDire/Rant-extra", "library": {"cmake": "extra::extra"}}"#,
        r#"{"name": "bridge", "repo": "KosmosisDire/rant-bridge"}"#,
    ]);

    // Three packages, each naming Rant its own way, one taking another library, and a folder
    // that uses none.
    write(&t.path("cam/CMakeLists.txt"), CAM);
    write(&t.path("py/pyproject.toml"), "[project]\nname = \"py\"\ndependencies = [\"rant-middleware>=0.0.15\"]\n");
    write(&t.path("hmi/hmi.csproj"), "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"0.0.14\" />\n  </ItemGroup>\n</Project>\n");
    write(&t.path("geo/CMakeLists.txt"), "project(geo C)\nCPMAddPackage(NAME extra GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git GIT_TAG v1.2.0)\n");
    write(&t.path("tool/CMakeLists.txt"), "project(tool C)\nadd_executable(tool tool.c)\n");

    let r = t.rant(&["ls", "libraries"]).ok();
    expect_match(&r.out, r"cam +rant +cmake +0[.]0[.]16 +CPM cam/CMakeLists[.]txt\n");
    expect_match(&r.out, r"geo +extra +cmake +1[.]2[.]0 +CPM geo/CMakeLists[.]txt\n");
    expect_match(&r.out, r"hmi +rant +csharp +0[.]0[.]14 +PackageReference hmi/hmi[.]csproj\n");
    expect_match(&r.out, r"py +rant +python +0[.]0[.]15 +pyproject py/pyproject[.]toml\n");
    expect_match(&r.out, r"py +rant +python +none +no venv yet\n");
    expect_no_match(&r.out, "tool");
    let r = t.rant(&["ls", "libraries", "extra"]).ok();
    expect_match(&r.out, r"^LIBRARIES\n  geo +extra +cmake +1[.]2[.]0 +CPM geo/CMakeLists[.]txt\n$");
    let r = t.rant(&["ls", "libraries", "--json"]).ok();
    expect_match(&r.out, r#""library": "rant",\n *"folder": [^\n]*cam",\n *"kind": "cmake",\n *"how": "CPM""#);
    let r = t.rant(&["ls", "libraries", "nothing"]).ok();
    expect_match(&r.err, "nothing here uses a library yet");

    let r = t.rant(&["add", "rant", "nowhere"]).fails();
    expect_match(&r.err, "no folder `nowhere`");
    let r = t.rant(&["add", "a", "b", "c"]).fails();
    expect_match(&r.err, "add takes a library and one folder at most");
    let r = t.rant(&["add", "Not a name"]).fails();
    expect_match(&r.err, "is no library name");

    // A library is added and a package installed, each refusing the other, and a library comes
    // only from a repo's releases.
    write(&t.path("geometry/package.rant"), "name = \"geometry\"\nlibrary {\n  cmake = \"geometry\"\n}\n");
    write(&t.path("kit/package.rant"), "name = \"kit\"\n");
    write(&format!("{home}/config.rant"), &format!("paths = [\"{s}/geometry\", \"{s}/kit\"]\n", s = t.scratch));
    let r = t.rant(&["install", "geometry"]).fails();
    expect_match(&r.err, "geometry is a library, add it to a package with `rant add geometry`");
    let r = t.rant(&["add", "kit"]).fails();
    expect_match(&r.err, "kit is a package, install it with `rant install kit`");
    let r = t.rant(&["add", "geometry", "tool"]).fails();
    expect_match(&r.err, "geometry is a folder in config[.]rant, and a library comes from a repo's releases");

    // The rest reads Rant's package.rant and its releases from GitHub, so it needs the network.
    write(&format!("{home}/config.rant"), "repos = [\"KosmosisDire/Rant\"]\n");
    let r = t.rant(&["add", "rant", "tool", "--version", "0.0.17"]).any();
    if r.code != 0 && pattern("cannot reach|rate limit").is_match(&r.err) {
        eprintln!("SKIPPED: GitHub is not reachable: {}", r.err);
        return;
    }
    assert_eq!(r.code, 0, "rant add rant tool exited {}\n{}", r.code, r.err);
    expect_match(&r.out, r"^rant 0[.]0[.]17\n  tool cmake: added to tool/CMakeLists[.]txt\n");
    let tool = read(&t.path("tool/CMakeLists.txt"));
    expect_match(&tool, r"FetchContent_Declare[(]rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant[.]git\n  GIT_TAG v0[.]0[.]17\n");
    expect_match(&tool, r"add_executable[(]tool tool[.]c[)]\ntarget_link_libraries[(]tool PRIVATE rant::rant_host[)]\n");

    // add moves a library a folder names already, and only a version asked for moves it back.
    let r = t.rant(&["add", "rant", "cam", "--version", "0.0.17"]).ok();
    expect_match(&r.out, r"cam cmake: 0[.]0[.]16 -> 0[.]0[.]17 in cam/CMakeLists[.]txt\n");
    expect_match(&read(&t.path("cam/CMakeLists.txt")), r"  GIT_TAG v0[.]0[.]17[)]");
    let r = t.rant(&["add", "rant", "cam", "--version", "0.0.17"]).ok();
    expect_match(&r.err, "cam uses rant 0[.]0[.]17 already");
    let r = t.rant(&["add", "--version", "0.0.99"]).fails();
    expect_match(&r.err, "has no release 0[.]0[.]99");

    // update moves every use in the workspace to the latest release, after listing it. The
    // question is answered yes, since stdin is not a terminal.
    write(&t.path("ws/workspace.rant"), "");
    write(&t.path("ws/arm/CMakeLists.txt"), "project(arm C)\nCPMAddPackage(NAME rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG v0.0.17)\n");
    let r = t.rant(&["update", "rant"]).cwd(&t.path("ws")).ok();
    expect_match(&r.out, r"^rant ([0-9.]+)\n  arm cmake: 0[.]0[.]17 -> [0-9.]+ in arm/CMakeLists[.]txt\n.*updated 1 package");
    let r = t.rant(&["update", "rant"]).cwd(&t.path("ws")).ok();
    expect_match(&r.out, "everything here is at [0-9.]+ or newer");

    let r = t.rant(&["ls", "libraries", "--available"]).ok();
    expect_match(&r.out, "rant +KosmosisDire/Rant +cmake python csharp rust");

    // A release that carries a skill gives it to the agents found, and remove takes it back.
    mkdir(&format!("{}/.claude", t.home));
    let r = t.rant(&["add", "rant", "cam", "--version", "0.0.40"]).ok();
    expect_match(&r.err, "Also give the skill of rant to Claude Code");
    expect_match(&read(&format!("{}/.claude/skills/rant/SKILL.md", t.home)), "name: rant");
    t.rant(&["remove", "rant"]).ok();
    assert!(!exists(&format!("{}/.claude/skills/rant", t.home)), "rant remove left the skill of rant linked");
}
