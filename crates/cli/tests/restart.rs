mod common;
use common::*;

#[test]
fn restart() {
    let t = Test::new("restart", 154);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");

    let r = t.rant(&["run", "nav"]).ok();
    let first_pid = capture_group(&r.out, "lidar +started +pid ([0-9]+)");

    // A node comes back under its name with a new process.
    let r = t.rant(&["restart", "lidar"]).ok();
    let pid = capture_group(&r.out, "lidar +restarted +pid ([0-9]+)");
    assert!(pid.is_some() && pid != first_pid, "lidar was not restarted as a new process:\n{}", r.out);
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  nav:\n    planner\n    base:\n      lidar\n      odom\n\nNOT RUNNING");

    // A group restarts as it is configured now: an edit is picked up, a dropped node stops.
    let nav = read(&t.path("nav.run")).replace(r#""--fn", "speed_${param.speed}""#, r#""--fn", "turn""#);
    write(&t.path("nav.run"), &nav);
    let r = t.rant(&["restart", "nav"]).ok();
    expect_match(&r.out, r"lidar +restarted +pid [0-9]+.*\n.*odom +restarted +pid [0-9]+.*\n.*planner +restarted +pid [0-9]+");
    let r = t.rant(&["info", "node", "planner"]).ok();
    expect_match(&r.out, r#""--fn", turn\]"#);

    // Asked to restart everything, it asks first.
    let r = t.rant(&["restart"]).ok();
    expect_match(&r.err, r"roots: group nav range=30 speed=1\n");
    expect_match(&r.err, r"Restart all 3 nodes[?] \[y/N\] y");

    let r = t.rant(&["restart", "nowhere"]).fails();
    expect_match(&r.err, "`nowhere` is not running, starting it");
    expect_match(&r.err, "no node named `nowhere`");

    // What is not running starts.
    let r = t.rant(&["restart", "nav", "speed=9"]).ok();
    expect_match(&r.err, "group `nav` is not running, starting it");
    expect_match(&r.out, "planner");
    t.rant(&["stop", "nav", "speed=9"]).ok();
    let r = t.rant(&["restart", "sensor"]).ok();
    expect_match(&r.err, "`sensor` is not running, starting it");
    expect_match(&r.out, "sensor +started +pid [0-9]+");

    // A restart builds the node's package once it has stopped. A failed build starts the last
    // build again and fails the restart.
    let package = t.path("demo/package.rant");
    let demo = read(&package);
    write(&package, &format!("{demo}build = \"cmake -E echo rebuilt-demo\"\n"));
    let r = t.rant(&["restart", "sensor"]).ok();
    expect_match(&r.err, r"building demo\nrebuilt-demo\n");
    expect_match(&r.out, "sensor +restarted +pid [0-9]+");
    write(&package, &format!("{demo}build = \"cmake -E false\"\n"));
    let r = t.rant(&["restart", "sensor"]).fails();
    expect_match(&r.err, "building demo failed: .* exited 1, starting the last build");
    expect_match(&r.out, "sensor +restarted +pid [0-9]+");
    let r = t.rant(&["restart", "sensor", "--no-build"]).ok();
    expect_no_match(&r.err, "building demo");
    write(&package, &demo);

    // A node that is not running, one that crashed say, is built before it starts too, as run
    // builds what it starts.
    t.rant(&["stop", "sensor"]).ok();
    write(&package, &format!("{demo}build = \"cmake -E echo rebuilt-demo\"\n"));
    let r = t.rant(&["restart", "sensor"]).ok();
    expect_match(&r.err, "`sensor` is not running, starting it");
    expect_match(&r.err, r"building demo\nrebuilt-demo\n");
    expect_match(&r.out, "sensor +started +pid [0-9]+");
    write(&package, &demo);

    // A node type means the node running under the name it starts with, never a copy beside it.
    let r = t.rant(&["restart", "demo/sensor"]).ok();
    expect_match(&r.out, "sensor +restarted +pid [0-9]+");
    let r = t.rant(&["restart", "node", "demo/sensor"]).ok();
    expect_match(&r.out, "sensor +restarted +pid [0-9]+");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_no_match(&r.out, "sensor_1");
    let r = t.rant(&["restart", "demo/sensor", "range=1"]).fails();
    expect_match(&r.err, "a node takes no key=value");

    // From another workspace it finds the node where it runs, as the node's name does.
    mkdir(&t.other);
    copy_into(&t.path("demo"), &t.other);
    write(&format!("{}/workspace.rant", t.other), "");
    let r = t.rant(&["restart", "demo/sensor"]).cwd(&t.other).ok();
    expect_match(&r.err, "`sensor` runs in");
    expect_match(&r.out, "sensor +restarted +pid [0-9]+");

    // With none under that name it starts one, as with another type under that name.
    t.rant(&["stop", "sensor"]).ok();
    let r = t.rant(&["restart", "demo/sensor"]).ok();
    expect_match(&r.err, "`demo/sensor` is not running, starting it");
    expect_match(&r.out, "sensor +started +pid [0-9]+");
    t.rant(&["stop", "sensor"]).ok();
    write(&package, &format!("{demo}node \"other\" {{\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}}\n"));
    write(&t.path("Stand.run"), "node \"demo/other\" {\n  name = \"sensor\"\n}\n");
    t.rant(&["run", "Stand"]).ok();
    let r = t.rant(&["restart", "demo/sensor"]).ok();
    expect_match(&r.out, "sensor_1 +started +pid [0-9]+");
    t.rant(&["stop", "Stand"]).ok();
    t.rant(&["stop", "sensor_1"]).ok();
    remove(&t.path("Stand.run"));
    write(&package, &demo);

    // Roots sharing a node start it once. A second start leaves the first running untracked, which
    // stays on the mesh after a stop.
    t.rant(&["run", "pick"]).ok();
    let r = t.rant(&["restart"]).ok();
    expect_match(&r.out, r"\n4 restarted, all running");
    t.rant(&["run", "nav", "speed=9"]).ok();
    let r = t.rant(&["restart", "nav"]).ok();
    expect_match(&r.out, r"\n3 restarted, all running");

    t.rant(&["stop"]).ok();
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  [(]none[)]");
}
