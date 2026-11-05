mod common;
use common::*;

#[test]
fn groups() {
    let t = Test::new("groups", 145);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    let d = t.domain;

    // A group explains itself, and a dry run shows the resolved plan with absolute paths.
    let r = t.rant(&["run", "nav", "--help"]).ok();
    expect_match(&r.out, "Usage: rant run nav");
    expect_match(&r.out, "Drive around");
    expect_match(&r.out, "speed=<float> +default 1");
    expect_match(&r.out, "range=<int> +default 30 +How far the lidar looks");
    let r = t.rant(&["run", "nav", "range=5", "--dry-run", "--json"]).ok();
    expect_match(&r.out, r#""name": "lidar""#);
    expect_match(&r.out, r#""range_5""#);
    expect_match(&r.out, r#""cwd": "[A-Za-z]*:?/[^"]*/demo""#);

    // Bad params never start anything.
    let r = t.rant(&["run", "nav", "speed=fast"]).fails();
    expect_match(&r.err, "param `speed` takes a float");
    let r = t.rant(&["run", "nav", "colour=red"]).fails();
    expect_match(&r.err, "group `nav` has no param `colour`");
    let r = t.rant(&["run", "nowhere"]).fails();
    expect_match(&r.err, "no group named `nowhere`, see `rant ls groups`");

    // Two groups share lidar and odom. A group named against the convention is noted.
    let r = t.rant(&["run", "nav"]).ok();
    expect_match(&r.out, r"^nav  range=30 speed=1\n\n.*lidar +started.*\n.*odom +started.*\n.*planner +started.*\n\n3 started, all running after 2 s\n$");
    expect_match(&r.err, "rename group `nav` to `Nav`");
    let r = t.rant(&["run", "nav"]).ok();
    expect_match(&r.out, r"lidar +already running\n.*odom +already running\n.*planner +already running\n\n3 already running\n");
    let r = t.rant(&["run", "pick"]).ok();
    expect_match(&r.out, r"lidar +already running +shared with nav\n.*odom +already running +shared with nav\n.*arm +started");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  nav:\n    planner\n    base:\n      lidar\n      odom\n  pick:\n    arm\n    base:\n      lidar\n      odom\n\nNOT RUNNING");
    let r = t.rant(&["ls", "entities"]).ok();
    expect_match(&r.out, "scan +topic");
    let r = t.rant(&["info", "lidar"]).ok();
    expect_match(&r.out, r"roots: +\[group nav range=30 speed=1, group pick\]\n");

    // Stopping one group leaves the shared nodes up.
    let r = t.rant(&["stop", "nav"]).ok();
    expect_match(&r.out, r"planner +stopped\n.*odom +kept running +still needed by pick\n.*lidar +kept running");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  pick:\n    arm\n    base:\n      lidar\n      odom\n\nNOT RUNNING");
    let r = t.rant(&["sub", "scan", "--count", "1"]).ok();
    expect_match(&r.out, "seq");

    // A node of the same name that rant did not start can stand in, after a question.
    mkdir(&t.other);
    copy_into(&t.path("demo"), &t.other);
    write(&format!("{}/workspace.rant", t.other), "");
    write(&format!("{}/demo/package.rant", t.other), "name = \"demo\"\nnode \"planner\" {\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}\n");
    t.rant(&["run", "node", "planner"]).cwd(&t.other).ok();
    let r = t.rant(&["run", "nav"]).ok();
    expect_match(&r.err, r"`planner` is already running on the mesh, but rant did not start it. Use it for nav\? \[y/N\] y");
    expect_match(&r.out, "planner +already running +started outside rant");
    let r = t.rant(&["stop", "nav"]).ok();
    expect_no_match(&r.out, "planner");
    let r = t.rant(&["ls", "nodes"]).cwd(&t.other).ok();
    expect_match(&r.out, r"NODES\n  planner\n");

    // Without the mesh only what this workspace started, so not the other one's planner, and no
    // word on whether a node joined.
    let r = t.rant(&["ls", "nodes", "--no-mesh", "--json"]).ok();
    expect_match(&r.out, &format!(r#""domain": {d},"#));
    expect_match(&r.out, &format!(r#""name": "arm",\n +"type": "demo/sensor",\n +"domain": {d},\n +"group": "pick"\n"#));
    expect_match(&r.out, &format!(r#""name": "lidar",\n +"type": "demo/sensor",\n +"domain": {d},\n +"group": "base"\n"#));
    expect_match(&r.out, r#""group": "pick""#);
    expect_match(&r.out, r#""nodes": \[\]"#);
    expect_no_match(&r.out, "on_mesh");
    let r = t.rant(&["ls", "topics", "--no-mesh"]).fails();
    expect_match(&r.err, "--no-mesh goes with `rant ls nodes`");
    t.rant(&["stop", "planner"]).cwd(&t.other).ok();

    // Stopping the last group stops everything, last started first.
    let r = t.rant(&["stop", "pick"]).ok();
    expect_match(&r.out, r"arm +stopped\n.*odom +stopped\n.*lidar +stopped\n");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  [(]none[)]");

    // A group that runs only inside another restarts and stops just its own nodes there.
    t.rant(&["run", "pick"]).ok();
    let r = t.rant(&["restart", "base"]).ok();
    expect_match(&r.out, r"^base *\n");
    expect_match(&r.out, "lidar +restarted");
    expect_match(&r.out, "odom +restarted");
    let r = t.rant(&["stop", "base"]).ok();
    expect_match(&r.out, r"odom +stopped +inside pick\n.*lidar +stopped +inside pick\n");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  pick:\n    arm\n\nNOT RUNNING");
    let r = t.rant(&["stop", "pick"]).ok();
    expect_match(&r.out, "arm +stopped");

    // A node started on its own and then taken into groups stops with the last group holding it.
    write(&t.path("Solo.run"), "node \"demo/sensor\" {}\n");
    write(&t.path("Duo.run"), "node \"demo/sensor\" {}\n");
    t.rant(&["run", "node", "sensor"]).ok();
    let r = t.rant(&["run", "Solo"]).ok();
    expect_match(&r.out, r"sensor +already running +started on its own\n");
    let r = t.rant(&["run", "Duo"]).ok();
    expect_match(&r.out, r"sensor +already running +started on its own, shared with Solo\n");
    let r = t.rant(&["stop", "Solo"]).ok();
    expect_match(&r.out, r"sensor +kept running +started on its own, still needed by Duo\n");
    let r = t.rant(&["stop", "Duo"]).ok();
    expect_match(&r.out, "sensor +stopped");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  [(]none[)]");

    // A group and a node of one name, as packages ship them, are one name without case or
    // underscores. The group wins, a stop takes what runs, and `node` picks the node.
    write(&t.path("Sensor.run"), "node \"demo/sensor\" {}\n");
    let r = t.rant(&["run", "sensor"]).ok();
    expect_match(&r.out, r"Sensor\n\n.*sensor +started");
    t.rant(&["log", "SENSOR"]).ok();
    let r = t.rant(&["stop", "SENSOR"]).ok();
    expect_match(&r.out, r"Sensor\n\n.*sensor +stopped");
    let r = t.rant(&["run", "node", "Sensor"]).ok();
    expect_match(&r.out, "sensor +started");
    let r = t.rant(&["stop", "sensor"]).ok();
    expect_match(&r.out, "sensor +stopped");
    t.rant(&["run", "Sensor"]).ok();
    let r = t.rant(&["stop", "node", "sensor"]).ok();
    expect_match(&r.err, "`sensor` belongs to the running group Sensor. Stop it anyway[?]");
    expect_match(&r.out, "sensor +stopped");
    t.rant(&["stop", "Sensor"]).ok();

    // A bare stop lists everything rant started and stops it all after a question.
    t.rant(&["run", "nav"]).ok();
    t.rant(&["run", "pick"]).ok();
    let r = t.rant(&["stop"]).ok();
    expect_match(&r.out, r"groups: nav, pick\nnodes:  lidar, odom, planner, arm\n");
    expect_match(&r.err, r"Stop all 4 nodes[?] \[y/N\] y");
    expect_match(&r.out, r"arm +stopped\n.*planner +stopped\n.*odom +stopped\n.*lidar +stopped\n");
    let r = t.rant(&["stop"]).ok();
    expect_match(&r.out, "nothing started here is running");

    // Placement: prefixes join from the workspace in, and the node prefix is part of the name.
    write(&t.path("workspace.rant"), "prefix = \"plant\"\n");
    write(&t.path("cell.run"), "node_prefix = \"left\"\nprefix      = \"left\"\nnode \"sensor\" {\n  name = \"lidar\"\n}\n");
    let r = t.rant(&["run", "cell", "--dry-run", "--json"]).ok();
    expect_match(&r.out, r#""name": "left/lidar""#);
    expect_match(&r.out, r#""RANT_PREFIX": "plant/left""#);
    let r = t.rant(&["run", "cell"]).ok();
    expect_match(&r.out, "left/lidar +started");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"cell:\n    left/lidar\n");
    let r = t.rant(&["stop", "cell"]).ok();
    expect_match(&r.out, "left/lidar +stopped");

    // A name runs wherever rant started it on this machine. A group another workspace runs is
    // restarted, read and stopped from here, the runs of one group in a workspace restart and stop
    // together, and a name two workspaces run needs a pick, so without a terminal it fails.
    write(&format!("{}/Plan.run", t.other), "node \"demo/planner\" {}\n");
    t.rant(&["run", "Plan"]).cwd(&t.other).ok();
    let r = t.rant(&["restart", "plan"]).ok();
    expect_match(&r.err, r"`plan` runs in [^\n]*-other");
    expect_match(&r.out, "planner +restarted");
    t.rant(&["log", "PLAN"]).ok();
    let r = t.rant(&["stop", "Plan"]).ok();
    expect_match(&r.err, r"`Plan` runs in [^\n]*-other");
    expect_match(&r.out, "planner +stopped");
    let r = t.rant(&["ls", "nodes"]).cwd(&t.other).ok();
    expect_match(&r.out, r"NODES\n  [(]none[)]");

    write(&t.path("Cam.run"), "param \"side\" {}\nnode \"demo/sensor\" {\n  name = \"cam_${param.side}\"\n}\n");
    t.rant(&["run", "Cam", "side=left"]).ok();
    t.rant(&["run", "Cam", "side=right"]).ok();
    let r = t.rant(&["restart", "cam"]).ok();
    expect_match(&r.out, "cam_left +restarted");
    expect_match(&r.out, "cam_right +restarted");
    expect_no_match(&r.err, "Restart all");
    let r = t.rant(&["stop", "Cam"]).ok();
    expect_match(&r.out, "cam_right +stopped");
    expect_match(&r.out, "cam_left +stopped");
    expect_no_match(&r.err, "Stop all");

    write(&t.path("Plan.run"), "node \"demo/sensor\" {\n  name = \"plan_sensor\"\n}\n");
    t.rant(&["run", "Plan"]).cwd(&t.other).ok();
    t.rant(&["run", "Plan"]).ok();
    let r = t.rant(&["stop", "Plan"]).fails();
    expect_match(&r.err, r"`Plan` runs in 2 workspaces: [^\n]*, pick one in a terminal");
    t.rant(&["stop"]).cwd(&t.other).ok();
    let r = t.rant(&["stop", "Plan"]).ok();
    expect_match(&r.out, "plan_sensor +stopped");
}
