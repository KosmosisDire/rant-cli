mod common;
use common::*;

#[test]
fn installed() {
    let t = Test::new("installed", 158);

    // An installed package is a folder in rant's home holding its package.rant, its program and
    // the groups beside them, as rant install leaves it.
    let pkg = format!("{}/packages/tool", t.rant_home);
    t.install_test_node("home/.rant/packages/tool/bin", "test_node");
    let cmake = cmake();
    write(
        &format!("{pkg}/package.rant"),
        &format!(
            "description = \"Answers questions\"\nnode \"tool\" {{\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}}\nnode \"idle\" {{\n  run = [\"{cmake}\", \"-E\", \"sleep\", \"60\"]\n  expect_mesh = false\n}}\n"
        ),
    );
    write(&format!("{pkg}/Tool.run"), "node \"tool\" {}\n");
    let window = test_window();
    if let Some(window) = &window {
        copy_file(window, &format!("{pkg}/bin/test_window.exe"));
        append(&format!("{pkg}/package.rant"), "node \"window\" {\n  run = \"./bin/test_window\"\n  expect_mesh = false\n}\n");
    }

    // Outside every workspace the installed packages stand alone, run from rant's home.
    let r = t.rant(&["ls", "packages"]).ok();
    expect_match(&r.out, r"^PACKAGES\n  tool +package[.]rant +installed +");
    let r = t.rant(&["ls", "groups"]).ok();
    expect_match(&r.out, r"^GROUPS\n  tool/Tool");
    let r = t.rant(&["info", "package", "tool"]).ok();
    expect_match(&r.out, r"\ndescription: Answers questions\n");
    let r = t.rant(&["run", "Tool"]).ok();
    expect_match(&r.out, "tool +started");
    let r = t.rant(&["log", "tool"]).ok();
    expect_match(&r.out, r"^ready test_node\n$");
    assert!(exists(&format!("{}/logs/tool.log", t.rant_home)), "the log of a node run outside a workspace belongs in rant's home");
    let r = t.rant(&["stop", "Tool"]).ok();
    expect_match(&r.out, "tool +stopped");

    // A program that never joins the mesh is up once it has started.
    let r = t.rant(&["run", "node", "idle"]).ok();
    expect_match(&r.out, "idle +started");
    let r = t.rant(&["ls", "nodes", "--json"]).ok();
    expect_match(&r.out, r#""name": "idle""#);
    expect_no_match(&r.out, "on_mesh");
    let r = t.rant(&["stop", "idle"]).ok();
    expect_match(&r.out, "idle +stopped");

    // A Windows GUI program has no console for Ctrl-Break, so rant asks it to close its windows.
    if window.is_some() {
        let r = t.rant(&["run", "node", "window"]).ok();
        expect_match(&r.out, "window +started");
        let r = t.rant(&["stop", "window"]).ok();
        expect_match(&r.out, "window +stopped");
        let r = t.rant(&["log", "window"]).ok();
        expect_match(&r.out, r"^ready test_window\nstopped\n$");
    }

    let r = t.rant(&["build"]).fails();
    expect_match(&r.err, "no workspace here");

    // A workspace sees them after its own packages, and never builds one.
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    let r = t.rant(&["ls", "packages"]).ok();
    expect_match(&r.out, r"\n  demo +package[.]rant +no library +demo\n  tool +package[.]rant +installed +");
    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_no_match(&r.out, "tool");
    let r = t.rant(&["build", "tool"]).fails();
    expect_match(&r.err, "`tool` is installed, it came built");
    let r = t.rant(&["run", "node", "tool"]).ok();
    expect_match(&r.out, "tool +started");
    t.rant(&["stop", "tool"]).ok();

    // A package of the workspace hides the installed one of the same name.
    write(&t.path("tool/package.rant"), "node \"mine\" {\n  run = \"mine\"\n}\n");
    let r = t.rant(&["ls", "packages", "tool", "--json"]).ok();
    expect_match(&r.out, r#""installed": false"#);
    expect_no_match(&r.out, r#""installed": true"#);
}
