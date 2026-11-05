mod common;
use common::*;

use serde_json::Value;

/// The rows `--json --lines` printed, each a JSON object.
fn rows(out: &str) -> Vec<Value> {
    out.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect()
}

fn states(rows: &[Value]) -> Vec<(String, String)> {
    rows.iter().map(|r| (r["name"].as_str().unwrap().to_string(), r["state"].as_str().unwrap().to_string())).collect()
}

fn pairs(want: &[(&str, &str)]) -> Vec<(String, String)> {
    want.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// The mesh names of the nodes this workspace's rant started that run now, as the manager reads them.
fn running(t: &Test) -> Vec<String> {
    let json: Value = serde_json::from_str(&t.rant(&["ls", "nodes", "--no-mesh", "--json"]).ok().out).unwrap();
    let runs = json["groups"].as_array().unwrap().iter().flat_map(|r| r["nodes"].as_array().unwrap().iter());
    let mut names: Vec<String> = runs.chain(json["nodes"].as_array().unwrap()).map(|n| n["name"].as_str().unwrap().to_string()).collect();
    names.sort();
    names.dedup();
    names
}

/// What the manager drives: the group list, then run, restart and stop of a group of two sleepers
/// and of one node, each a row a line as it changes.
#[test]
fn manager_rows() {
    let t = Test::new("manager_rows", 161);
    write(&t.path("workspace.rant"), "");
    let sleeper = "[\"./bin/test_node\", \"--life\", \"60\"]";
    write(
        &t.path("io/package.rant"),
        &format!(
            "node \"sleeper\" {{\n  run = {sleeper}\n  expect_mesh = false\n}}\nnode \"napper\" {{\n  run = {sleeper}\n  expect_mesh = false\n}}\n\
             node \"brief\" {{\n  run = [\"./bin/test_node\", \"--life\", \"0.3\"]\n  expect_mesh = false\n}}\n"
        ),
    );
    t.install_test_node("io/bin", "test_node");
    write(&t.path("io/Ft.run"), "description = \"Two sleepers\"\nnode \"io/sleeper\" {}\nnode \"io/napper\" {}\n");
    write(&t.path("Line.run"), "include \"io/Ft\" {}\n");

    let groups: Value = serde_json::from_str(&t.rant(&["ls", "groups", "--json"]).ok().out).unwrap();
    let group = |name: &str| groups.as_array().unwrap().iter().find(|g| g["name"] == name).unwrap().clone();
    let ft = group("io/Ft");
    assert_eq!((ft["description"].as_str(), ft["includes"].as_array().map(Vec::len)), (Some("Two sleepers"), Some(0)));
    let own: Vec<(&str, &str)> = ft["nodes"].as_array().unwrap().iter().map(|n| (n["name"].as_str().unwrap(), n["type"].as_str().unwrap())).collect();
    assert_eq!(own, [("sleeper", "io/sleeper"), ("napper", "io/napper")]);
    let line = group("Line");
    assert_eq!((line["description"].as_str(), line["includes"].clone(), line["nodes"].clone()), (Some(""), serde_json::json!(["io/Ft"]), serde_json::json!([])));

    // each node as it starts and as it comes up, nothing else on stdout or stderr
    let r = t.rant(&["run", "io/Ft", "--no-build", "-y", "--json", "--lines"]).ok();
    let run = rows(&r.out);
    assert_eq!(states(&run), pairs(&[("sleeper", "starting"), ("napper", "starting"), ("sleeper", "running"), ("napper", "running")]));
    assert_eq!(run[2]["status"], "started");
    assert!(run[2]["pid"].as_u64().is_some_and(|p| p > 0) && run[2]["log"].as_str().is_some_and(|l| l.ends_with("sleeper.log")), "{}", run[2]);
    assert_eq!(r.err, "");
    assert_eq!(running(&t), ["napper", "sleeper"]);

    let r = t.rant(&["restart", "node", "sleeper", "--no-build", "-y", "--json", "--lines"]).ok();
    let last = rows(&r.out).pop().unwrap();
    assert_eq!((last["name"].as_str(), last["state"].as_str(), last["status"].as_str()), (Some("sleeper"), Some("running"), Some("restarted")));
    assert!(std::path::Path::new(&t.path("logs/sleeper.log.1")).is_file(), "a restart starts a new log and keeps the last");

    let r = t.rant(&["stop", "sleeper", "-y", "--json", "--lines"]).ok();
    let last = rows(&r.out).pop().unwrap();
    assert_eq!((last["name"].as_str(), last["state"].as_str(), last["status"].as_str()), (Some("sleeper"), Some("stopped"), Some("stopped")));
    assert_eq!(running(&t), ["napper"]);

    t.rant(&["stop", "io/Ft", "-y", "--json", "--lines"]).ok();
    assert!(running(&t).is_empty());
    let r = t.rant(&["stop", "io/Ft", "-y", "--json", "--lines"]).fails();
    expect_match(&r.err, "not running");

    // a node that exits at once fails its row, exit 1 with nothing on stderr
    let r = t.rant(&["run", "node", "io/brief", "--no-build", "-y", "--json", "--lines"]).fails();
    let last = rows(&r.out).pop().unwrap();
    assert_eq!(last["state"], "failed");
    expect_match(last["status"].as_str().unwrap(), "^exited after ");
    assert_eq!((r.code, r.err.as_str()), (1, ""));
}
