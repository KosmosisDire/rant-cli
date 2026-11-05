mod common;
use common::*;

#[test]
fn build() {
    let t = Test::new("build", 146);
    t.use_fixture("build");

    // The dry run shows every edge with where it came from, the order and the exact commands.
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, r"DEPENDENCIES\n  app -> driver +depend in app/package.rant\n  driver -> sdk +find_package[(]sdk[)] in driver/CMakeLists.txt\n");
    expect_match(&r.out, r"BUILD ORDER\n  1. sdk\n +cmake -E echo building-sdk\n +cmake -E touch sdk.built\n  2. driver\n +cmake -S [.] -B build +[(]asks first[)]\n +cmake --build build\n  3. app\n");

    // Building app builds what it needs first, in the order the dry run showed. The configure
    // question is answered yes, since stdin is not a terminal.
    let r = t.rant(&["build", "app"]).ok();
    expect_match(&r.out, r"building sdk\n.*building-sdk\n.*building driver\n.*building-driver.*building app\n.*building-app\n.*built 3 packages");
    expect_match(&r.err, "driver is not configured yet. Configure it");
    let cache = t.path("driver/build/CMakeCache.txt");
    assert!(exists(&t.path("sdk/sdk.built")) && exists(&cache), "the builds did not run in their package directories");

    // Once configured, a build only builds.
    let r = t.rant(&["build", "driver"]).ok();
    expect_no_match(&r.err, "Configure");
    expect_match(&r.out, "built 2 packages");

    // A build folder CMake made in another place, as after a rename, is made again after a question.
    let moved = pattern(r"CMAKE_CACHEFILE_DIR:INTERNAL=[^\n]*").replace_all(&read(&cache), "CMAKE_CACHEFILE_DIR:INTERNAL=/elsewhere/Driver/build").into_owned();
    write(&cache, &moved);
    let r = t.rant(&["build", "driver", "--dry-run"]).ok();
    expect_match(&r.out, r"remove driver.build +[(]made in /elsewhere/Driver/build, asks first[)]\n +cmake -S [.] -B build\n");
    let r = t.rant(&["build", "driver"]).ok();
    expect_match(&r.err, "The build folder of driver was made in /elsewhere/Driver/build, which CMake cannot use here");
    expect_match(&r.out, "built 2 packages");

    // A clean build removes the build output first and configures again without a question.
    let leftover = t.path("driver/build/leftover.txt");
    write(&leftover, "");
    let r = t.rant(&["build", "driver", "--clean"]).ok();
    expect_match(&r.err, "sdk builds with its own commands, so rant removes nothing of it");
    expect_no_match(&r.err, "Configure it");
    expect_match(&r.out, "built 2 packages");
    assert!(!exists(&leftover) && exists(&cache), "the clean build did not start from an empty build folder");

    // The first failure stops the run and names the package and the command.
    write(&t.path("broken/package.rant"), "build  = [\"cmake -E false\", \"cmake -E echo never\"]\ndepend = [\"sdk\"]\n");
    let r = t.rant(&["build", "broken"]).fails();
    expect_match(&r.err, "building broken failed: `cmake -E false` exited 1");
    expect_no_match(&r.out, "never");

    // On Windows a batch file runs through cmd.exe, which must take every argument as data:
    // no %VAR% expansion and no & to start a second command.
    if cfg!(windows) {
        write(&t.path("batch/tool.cmd"), "@echo off\n>args.txt echo(%1\n>>args.txt echo(%2\n");
        write(&t.path("batch/package.rant"), "build = \"./tool.cmd %PATH% '&cmake -E touch pwned.txt'\"\n");
        t.rant(&["build", "batch"]).ok();
        expect_match(&read(&t.path("batch/args.txt")), r#"^"%PATH%"\r?\n"&cmake -E touch pwned.txt""#);
        assert!(!exists(&t.path("batch/pwned.txt")), "an argument ran as a command through cmd.exe");
    }

    let r = t.rant(&["build", "nope"]).fails();
    expect_match(&r.err, "no package named `nope`");
}
