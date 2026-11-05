mod common;
use common::*;

#[test]
fn new() {
    let t = Test::new("new", 151);
    // Rant is known as a library, as it is once the package database was read.
    t.cache_database(&[RANT_LIBRARY]);
    t.rant(&["init"]).ok();

    let r = t.rant(&["new", "group", "nav"]).ok();
    expect_match(&r.out, "created Nav[.]run");
    expect_match(&read(&t.path("Nav.run")), r#"description = "What Nav runs""#);
    let r = t.rant(&["new", "group", "nav"]).fails();
    expect_match(&r.err, "Nav[.]run exists already, nothing was written");

    // With no name a node or group takes the name of the folder it is made in.
    mkdir(&t.path("lidar"));
    let r = t.rant(&["new", "group"]).cwd(&t.path("lidar")).ok();
    expect_match(&r.out, "created Lidar[.]run");

    // A node takes the language of the package it is made in.
    write(&t.path("py/pyproject.toml"), "[project]\nname = \"py\"\ndependencies = [\"rant-middleware\"]\n");
    let r = t.rant(&["new", "node", "camera"]).cwd(&t.path("py")).ok();
    expect_match(&r.out, "created camera[.]py");
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, "py/camera +python +py/camera[.]py");

    let r = t.rant(&["new", "node", "arm"]).fails();
    expect_match(&r.err, "say which language with --lang");
    let r = t.rant(&["new", "node", "arm", "--lang", "cobol"]).fails();
    expect_match(&r.err, "unknown language `cobol`, use cpp, python, csharp or rust");
    let r = t.rant(&["new", "thing", "x"]).fails();
    expect_match(&r.err, "say what to make");
    let r = t.rant(&["new", "workspace", "demo"]).fails();
    expect_match(&r.err, "say which language with --lang");

    // A C++ node joins the CMake project around it, and needs one.
    let r = t.rant(&["new", "node", "arm", "--lang", "cpp"]).fails();
    expect_match(&r.err, "no CMakeLists.txt here or above to add `arm` to");
    write(&t.path("robot/CMakeLists.txt"), "project(robot CXX)\nadd_executable(base base.cpp)\n");
    mkdir(&t.path("robot/nodes"));
    let r = t.rant(&["new", "node", "arm"]).cwd(&t.path("robot/nodes")).any();
    expect_match(&r.out, r"created arm[.]cpp\nadded arm to [.][.]/CMakeLists[.]txt\n");
    expect_match(
        &read(&t.path("robot/CMakeLists.txt")),
        r"add_executable[(]base base[.]cpp[)]\nadd_executable[(]arm nodes/arm[.]cpp[)]\ntarget_link_libraries[(]arm PRIVATE rant::rant_host[)]\n",
    );
    let r = t.rant(&["new", "node", "arm", "--lang", "cpp"]).cwd(&t.path("robot")).fails();
    expect_match(&r.err, "target `arm` already, nothing was written");

    // A Rust node is a program file of the Cargo package around it, and needs one.
    if which("cargo").is_some() {
        let r = t.rant(&["new", "node", "motor", "--lang", "rust"]).fails();
        expect_match(&r.err, "no Cargo package here or above to add `motor` to");
        write(&t.path("drive/Cargo.toml"), "[package]\nname = \"drive\"\n\n[dependencies]\nrant-middleware = \"0.0.24\"\n");
        mkdir(&t.path("drive/src"));
        let r = t.rant(&["new", "node", "motor"]).cwd(&t.path("drive/src")).ok();
        expect_match(&r.out, r"created bin/motor[.]rs\n");
        let r = t.rant(&["build", "--dry-run"]).ok();
        expect_match(&r.out, "drive/motor +rust +drive/src/bin/motor[.]rs");
        let r = t.rant(&["new", "workspace", "rusty", "--lang", "rust"]).any();
        expect_match(
            &r.out,
            r"created rusty/src/bin/listener[.]rs\ncreated rusty/src/bin/talker[.]rs\ncreated rusty/workspace[.]rant\ncreated rusty/Rusty[.]run\ncreated rusty/[.]gitignore\ncreated rusty/Cargo[.]toml\n",
        );
    }

    // A workspace holds a talker, a listener and a group of both. Its CMake package knows its
    // nodes once built.
    let r = t.rant(&["new", "workspace", "demo", "--lang", "cpp"]).any();
    expect_match(
        &r.out,
        r"created demo/CMakeLists[.]txt\ncreated demo/src/listener[.]cpp\ncreated demo/src/talker[.]cpp\ncreated demo/workspace[.]rant\ncreated demo/Demo[.]run\n",
    );
    let r = t.rant(&["run", "Demo", "--dry-run"]).cwd(&t.path("demo")).fails();
    expect_match(&r.err, "no node named `talker`, `demo` is not built yet, run `rant build` first");

    // A package gets Rant through the same routine as rant add, which asks GitHub for the release.
    let r = t.rant(&["new", "package", "cam", "--lang", "cpp"]).any();
    expect_match(&r.out, r"created cam/CMakeLists[.]txt\ncreated cam/cam[.]cpp\n");
    if r.code != 0 && pattern("cannot reach|rate limit").is_match(&r.err) {
        eprintln!("SKIPPED: GitHub is not reachable: {}", r.err);
        return;
    }
    assert_eq!(r.code, 0, "rant new package cam exited {}\n{}", r.code, r.err);
    let cam = read(&t.path("cam/CMakeLists.txt"));
    expect_match(&cam, r"FetchContent_MakeAvailable[(]rant[)]");
    expect_match(&cam, r"add_executable[(]cam cam[.]cpp[)]\ntarget_link_libraries[(]cam PRIVATE rant::rant_host[)]\n");
    let r = t.rant(&["ls", "libraries"]).ok();
    expect_match(&r.out, "cam +rant +cmake +[0-9]+[.][0-9]+[.][0-9]+ +FetchContent");
}
