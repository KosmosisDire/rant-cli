mod common;
use common::*;

#[test]
fn rust() {
    let t = Test::new("rust", 148);
    if which("cargo").is_none() {
        eprintln!("SKIPPED: cargo is not installed");
        return;
    }
    t.use_fixture("rust");

    // Each program of the Cargo package is a node before any build, and builds with cargo build.
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, "motor/jog +rust +motor/src/bin/jog[.]rs");
    expect_match(&r.out, "motor/motor +rust +motor/src/main[.]rs");
    expect_match(&r.out, r"1. motor\n +cargo build\n");

    // It cannot start before it is built.
    let r = t.rant(&["run", "node", "motor"]).fails();
    expect_match(&r.err, "`motor/motor` is not built yet, run `rant build motor` first");

    // Built, it runs from the target folder, and the copies cargo keeps in it are no nodes. This
    // stand in never joins the mesh, so the run does not wait for it.
    let r = t.rant(&["build"]).ok();
    expect_match(&r.out, "built 1 package");
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, "motor/motor +rust +motor/target/debug/motor");
    let r = t.rant(&["ls", "nodes", "--no-mesh", "--json"]).ok();
    expect_no_match(&r.out, "motor-[0-9a-f]");
    let r = t.rant(&["run", "node", "motor", "--wait", "0"]).ok();
    expect_match(&r.out, "motor +started");
    let r = t.rant(&["stop", "motor"]).ok();
    expect_match(&r.out, r"motor +stopped\n");
    expect_match(&read(&t.path("logs/motor.log")), "ready motor");

    // rant ls libraries lists the use, and leaves a path one to its checkout.
    t.cache_database(&[RANT_LIBRARY]);
    let r = t.rant(&["ls", "libraries"]).ok();
    expect_match(&r.out, "motor +rant +rust +none +Cargo path");
}
