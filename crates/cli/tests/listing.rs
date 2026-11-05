mod common;
use common::*;

#[test]
fn listing() {
    let t = Test::new("listing", 153);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    // a group named like a node, so one name means two things
    write(&t.path("sensor.run"), "node \"demo/sensor\" {\n  name = \"one\"\n}\n");

    // Every kind word works singular, plural or short.
    for word in ["package", "packages", "pkg", "pkgs"] {
        let r = t.rant(&["ls", word]).ok();
        expect_match(&r.out, r"^PACKAGES\n  demo +package[.]rant +no library +demo\n$");
    }
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NOT RUNNING\n  demo/sensor +declared +");
    let r = t.rant(&["ls", "group"]).ok();
    expect_match(&r.out, r"  nav +speed=1 range=30 +Drive around\n");
    let r = t.rant(&["ls", "groups", "p"]).ok();
    expect_match(&r.out, r"^GROUPS\n  pick");
    expect_no_match(&r.out, "nav");
    let r = t.rant(&["ls", "nodes", "--json"]).ok();
    expect_match(&r.out, r#""not_running": \[\n +\{\n +"name": "demo/sensor""#);

    // ls alone lists every entity on the mesh, and its help names every kind.
    let r = t.rant(&["ls"]).ok();
    expect_match(&r.out, r"^ENTITIES\n  [(]none[)]\n$");
    let r = t.rant(&["ls", "-h"]).ok();
    expect_match(&r.out, r"^Usage: rant ls \[kind\] \[pattern\]\n");
    expect_match(&r.out, r"\nKinds:\n  nodes +running nodes");
    expect_match(&r.out, r"\n  groups +the groups of this workspace\n");
    let r = t.rant(&["ls", "nope"]).fails();
    expect_match(&r.err, "`nope` is not a kind ls lists, see `rant ls -h`");
    let r = t.rant(&["ls", "nodes", "a", "b"]).fails();
    expect_match(&r.err, "ls takes a kind and one pattern at most");

    // info explains workspace things, and says when a name means more than one.
    let r = t.rant(&["info", "package", "demo"]).ok();
    expect_match(&r.out, r"^package: +demo\n");
    expect_match(&r.out, r"nodes: +\[demo/sensor, demo/test_node\]");
    let r = t.rant(&["info", "group", "nav"]).ok();
    expect_match(&r.out, r"params:\n  speed: float, default 1\n  range: int, default 30\n");
    expect_match(&r.out, r"nodes:\n  lidar: +demo/sensor\n");
    let r = t.rant(&["info", "sensor"]).ok();
    expect_match(&r.err, "`sensor` names a node and a group, `rant info node sensor` shows only one");
    expect_match(&r.out, r"^node: +demo/sensor\n");
    expect_match(&r.out, r"groups: +\[base, nav, pick, sensor\]");
    expect_match(&r.out, r"\ngroup: +sensor\n");
    let r = t.rant(&["info", "node", "sensor"]).ok();
    expect_no_match(&r.out, "group:");
    let r = t.rant(&["info", "group", "nowhere"]).fails();
    expect_match(&r.err, "no group named `nowhere`");
    let r = t.rant(&["info", "thing", "nav"]).fails();
    expect_match(&r.err, "`thing` is not a kind");
}
