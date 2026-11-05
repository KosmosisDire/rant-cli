mod common;
use common::*;

#[test]
fn csharp() {
    let t = Test::new("csharp", 147);
    if which("dotnet").is_none() {
        eprintln!("SKIPPED: dotnet is not installed");
        return;
    }
    t.use_fixture("csharp");

    // The C# program is found from its project, and builds with dotnet build.
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, "panel/panel +csharp +panel/panel.csproj");
    expect_match(&r.out, r"1. panel\n +dotnet build .*/panel/panel.csproj\n");

    // It cannot start before it is built.
    let r = t.rant(&["run", "node", "panel"]).fails();
    expect_match(&r.err, "`panel/panel` is not built yet, run `rant build panel` first");

    // Built, it is found, started, listed and stopped like any other node. This stand in never
    // joins the mesh, so the run does not wait for it.
    let r = t.rant(&["build"]).ok();
    expect_match(&r.out, "built 1 package");
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, r"panel/panel +csharp +panel/bin/[^\n]*/panel");
    let r = t.rant(&["run", "node", "panel", "--wait", "0"]).ok();
    expect_match(&r.out, "panel +started");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  panel\n");
    let r = t.rant(&["stop", "panel"]).ok();
    expect_match(&r.out, r"panel +stopped\n");
    expect_match(&read(&t.path("logs/panel.log")), "ready panel");
}
