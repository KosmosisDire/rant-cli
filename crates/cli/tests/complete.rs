mod common;
use common::*;

const OPTS: &str = r#"param "target" {
  type    = string
  default = "red_bin"
  options = ["red_bin", "blue_bin"]
}
param "fast" {
  type    = bool
  default = false
}
node "demo/sensor" {
  name = "x"
}
"#;

#[test]
fn complete() {
    let t = Test::new("complete", 149);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    write(&t.path("opts.run"), OPTS);

    expect_match(&t.complete(&[], "r"), r"^record\nremove\nrestart\nrun\n$");
    expect_match(&t.complete(&["run"], ""), r"^base\nnav\nnode\nopts\npick\n$");
    expect_match(&t.complete(&["run", "node"], ""), r"^sensor\ntest_node\n$");

    // A group in a package goes by its bare name where that finds it.
    write(&t.path("demo/Drive.run"), "node \"demo/sensor\" {}\n");
    expect_match(&t.complete(&["run"], "D"), r"^Drive\n$");
    let r = t.rant(&["info", "group", "Drive"]).ok();
    expect_match(&r.out, "demo/Drive");
    remove(&t.path("demo/Drive.run"));

    // A group's params end in = and give way to their options.
    expect_match(&t.complete(&["run", "nav"], ""), r"^range=\nspeed=\n$");
    expect_match(&t.complete(&["run", "opts", "target=red_bin"], ""), r"^fast=\n$");
    expect_match(&t.complete(&["run", "opts"], "target="), r"^target=blue_bin\ntarget=red_bin\n$");
    expect_match(&t.complete(&["run", "opts"], "fast=t"), r"^fast=true\n$");

    expect_match(&t.complete(&[], "b"), r"^build\n$");
    expect_match(&t.complete(&["explore"], ""), "^$");

    expect_match(&t.complete(&["build"], ""), r"^demo\n$");
    expect_match(&t.complete(&["build"], "--d"), r"^--domain\n--dry-run\n$");

    // Nothing follows an option that takes a value, or a command that does not exist.
    expect_match(&t.complete(&["--domain"], ""), "^$");
    expect_match(&t.complete(&["nonsense"], ""), "^$");

    // The mesh comes from what earlier commands saw, a path one segment at a time.
    t.beside_node(&["--name", "cam", "--pub", "camera/left", "--var", "camera/rate"], &["ls", "entities"]);
    expect_match(&t.complete(&["sub"], ""), r"^camera/\n$");
    expect_match(&t.complete(&["sub"], "camera/"), r"^camera/left\ncamera/rate\n$");
    expect_match(&t.complete(&["pub"], "cam"), r"^camera/\n$");
    expect_match(&t.complete(&["info"], "c"), r"^cam\ncamera/\n$");

    // A full look at the mesh replaces the snapshot, so what left is no longer offered.
    t.rant(&["ls", "entities"]).ok();
    expect_match(&t.complete(&["sub"], ""), "^$");

    // setup writes the hook once, however often it runs.
    let r = t.rant(&["setup", "bash", "--print"]).ok();
    expect_match(&r.out, "complete -F _rant_complete rant");
    t.rant(&["setup", "bash"]).ok();
    let r = t.rant(&["setup", "bash"]).ok();
    expect_match(&r.out, "bash: tab completion loads from ");
    let bashrc = read(&format!("{}/.bashrc", t.home));
    assert_eq!(count_matches(&bashrc, "rant[.]bash"), 2, "expected one line loading the hook in .bashrc, got:\n{bashrc}");
    assert!(exists(&format!("{}/.rant/completions/rant.bash", t.home)), "setup wrote no hook");
    let r = t.rant(&["setup", "tcsh"]).fails();
    expect_match(&r.err, "unknown shell `tcsh`");

    // Setup points the agents found to the skill built into rant, and gives it when named.
    mkdir(&format!("{}/.claude", t.home));
    mkdir(&format!("{}/.cursor", t.home));
    let r = t.rant(&["setup", "bash"]).ok();
    expect_match(&r.err, "`rant setup skill` teaches Claude Code, Cursor to use rant-cli");
    t.rant(&["setup", "skill"]).ok();
    for link in [".claude/skills/rant-cli", ".agents/skills/rant-cli"] {
        let skill = read(&format!("{}/{link}/SKILL.md", t.home));
        assert!(skill.contains("name: rant-cli"), "{link} holds no rant-cli skill:\n{skill}");
    }
    let r = t.rant(&["setup", "bash"]).ok();
    assert!(!r.err.contains("rant setup skill"), "setup offers a skill already given:\n{}", r.err);
}
