mod common;
use common::*;

const KEEP: &str = r#"restart = "always"

node "demo/sensor" {
  name = "keeper"
  args = ["--life", "2", "--exit", "3"]
}

node "demo/sensor" {
  name    = "plain"
  restart = "never"
}
"#;

const FAIL: &str = r#"restart = "on-failure"

node "demo/sensor" {
  name = "good"
  args = ["--life", "2"]
}

node "demo/sensor" {
  name = "bad"
  args = ["--life", "2", "--exit", "1"]
}
"#;

#[test]
fn services() {
    let t = Test::new("services", 160);

    // Nodes with restart set run under the service process, which starts them again. Nothing
    // here writes a boot or login entry.
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    write(&t.path("Keep.run"), KEEP);
    write(&t.path("Fail.run"), FAIL);

    let r = t.rant(&["run", "Keep", "--dry-run"]).ok();
    expect_match(&r.out, r"keeper .*\n  restart  always\n");
    expect_no_match(&r.out, "plain .*restart");

    // The service process starts keeper, rant run plain, and both have a process at once.
    let r = t.rant(&["run", "Keep", "--watch", "0"]).ok();
    expect_match(&r.out, "keeper +started +pid [1-9]");
    expect_match(&r.out, "plain +started +pid [1-9]");
    let log = t.wait_for_service_log(r"started keeper in .*keeper exited with code 3, starting it again in 1 s.*started keeper again, restart 1\n");
    expect_no_match(&log, "plain");
    expect_match(&read(&t.path(".rant/state")), r#""restarts": [1-9]"#);

    // A node the service process starts, running already, has the environment of the rant run
    // that asked for it.
    write(
        &t.path("tools/package.rant"),
        &format!("name = \"tools\"\nnode \"dump\" {{\n  run = [\"{}\", \"-E\", \"environment\"]\n  expect_mesh = false\n}}\n", cmake()),
    );
    write(&t.path("Dump.run"), "restart = \"on-failure\"\nnode \"tools/dump\" {}\n");
    t.rant(&["run", "Dump", "--watch", "0"]).env("RANT_TEST_MARK", "from-the-shell").ok();
    t.wait_for_service_log("dump exited with code 0");
    expect_match(&read(&t.path("logs/dump.log")), "RANT_TEST_MARK=from-the-shell");
    t.rant(&["stop", "Dump"]).ok();

    // A stopped node stays stopped, whatever its restart says.
    t.rant(&["stop", "Keep"]).ok();
    let services_log = format!("{}/logs/services.log", t.rant_home);
    let before = read(&services_log);
    sleep(4.0);
    let after = read(&services_log);
    expect_no_match(&after[before.len()..], "started keeper");
    let r = t.rant(&["ls", "nodes", "--no-mesh"]).ok();
    expect_no_match(&r.out, "keeper");

    // on-failure starts again only what failed.
    t.rant(&["run", "Fail", "--watch", "0"]).ok();
    t.wait_for_service_log("good exited with code 0, and its restart is on-failure");
    t.wait_for_service_log("bad exited with code 1, starting it again");
    t.rant(&["stop", "Fail"]).ok();

    // With nothing left to watch it ends on its own.
    t.wait_for_service_log(r"stopped, nothing to watch\n$");

    // rant enable refuses what it cannot run before it keeps anything or writes an entry, and a
    // dry run keeps nothing.
    let r = t.rant(&["enable"]).fails();
    expect_match(&r.err, "no services in workspace.rant, `rant enable <group>` adds one");
    let r = t.rant(&["enable", "nowhere"]).fails();
    expect_match(&r.err, "no group named `nowhere`");
    let r = t.rant(&["enable", "sensor"]).fails();
    expect_match(&r.err, "`sensor` is a node and a service is a group, `rant new group` makes one for it");
    let r = t.rant(&["enable", "node", "sensor"]).fails();
    expect_match(&r.err, "a service is a group");
    let r = t.rant(&["enable", "Keep", "--dry-run"]).ok();
    expect_match(&r.out, "keeper");
    let r = t.rant(&["ls", "services"]).ok();
    expect_match(&r.out, r"SERVICES\n  [(]none[)] +not enabled +[.]\n");
    expect_no_match(&read(&t.path("workspace.rant")), "service");

    // A workspace lists its services, and a bare run starts each as an ordinary run of its group,
    // as every boot will once the workspace is enabled. One that does not plan is named at its line.
    write(&t.path("Plain.run"), "node \"demo/sensor\" {\n  name = \"steady\"\n}\n");
    write(&t.path("Desk.run"), "param \"seat\" {}\nnode \"demo/sensor\" {\n  name = \"desk_${param.seat}\"\n}\n");
    write(&t.path("workspace.rant"), "service \"Plain\" {}\nservice \"Desk\" {\n  seat = 1\n}\n");
    let r = t.rant(&["run"]).fails();
    expect_match(&r.err, "workspace.rant:3:3: param `seat` is string, not an int");
    expect_match(&r.out, "steady +started");
    write(&t.path("workspace.rant"), "service \"Plain\" {}\n\n# where the operator sits\nservice \"Desk\" {\n  seat = \"left\"\n}\n");
    let r = t.rant(&["ls", "services"]).ok();
    expect_match(&r.out, r"SERVICES\n  Plain +not enabled +running[^\n]*\n  Desk seat=left +not enabled +stopped");
    let r = t.rant(&["run", "--dry-run"]).ok();
    expect_match(&r.out, "desk_left");
    let r = t.rant(&["run"]).ok();
    expect_match(&r.out, "steady +already running");
    expect_match(&r.out, "desk_left +started");

    // Outside every workspace the services are those of services.rant in rant's home.
    let outside = format!("{}-outside", t.scratch);
    mkdir(&outside);
    write(&format!("{}/services.rant", t.rant_home), "service \"Nowhere\" {}\n");
    let r = t.rant(&["run"]).cwd(&outside).fails();
    expect_match(&r.err, "services.rant:1:1: no group named `Nowhere`");
    remove(&format!("{}/services.rant", t.rant_home));
    let r = t.rant(&["run"]).cwd(&outside).fails();
    expect_match(&r.err, "say which group to run");

    // What rant enable keeps, written here by hand, since rant enable writes the real boot and
    // login entries. An entry without a start starts at boot, and an older rant's entry for one
    // group is passed over. The service process starts each workspace's services as the entries
    // would.
    let r = t.rant(&["stop", "-y"]).ok();
    expect_match(&r.out, "steady +stopped");
    mkdir(&t.other);
    copy_into(&t.path("demo"), &t.other);
    write(&format!("{}/workspace.rant", t.other), "service \"Far\" {}\n");
    write(&format!("{}/Far.run", t.other), "node \"demo/sensor\" {\n  name = \"far\"\n}\n");
    write(
        &format!("{}/services.json", t.rant_home),
        &format!(
            r#"{{ "enabled": [
  {{ "folder": "{s}", "start": "login", "domain": {d} }},
  {{ "folder": "{o}", "domain": {d} }},
  {{ "folder": "{s}", "kind": "group", "name": "Plain", "words": [] }}
] }}
"#,
            s = t.scratch,
            o = t.other,
            d = t.domain
        ),
    );
    let r = t.rant(&["ls", "services"]).ok();
    expect_match(&r.out, r"SERVICES\n  Plain +at login +stopped[^\n]*\n  Desk seat=left +at login +stopped[^\n]*\n  Far +at boot +stopped[^\n]*-other\n");

    t.rant_unwatched(&["__services", "--login", &t.rant_home]);
    let log = t.wait_for_service_log("at login, starting the services of ");
    expect_no_match(&log, r"starting the services of [^\n]*-other");
    t.rant_unwatched(&["__services", "--boot", &t.rant_home]);
    t.wait_for_service_log(r"at boot, starting the services of [^\n]*-other");
    let (mut here, mut there) = (String::new(), String::new());
    for _ in 0..=40 {
        here = t.rant(&["ls", "nodes", "--no-mesh"]).ok().out;
        there = t.rant(&["ls", "nodes", "--no-mesh"]).cwd(&t.other).ok().out;
        if here.contains("steady") && here.contains("desk_left") && there.contains("far") {
            break;
        }
        sleep(0.5);
    }
    expect_match(&here, r"Plain:\n +steady\n");
    expect_match(&here, r"Desk:\n +desk_left\n");
    expect_match(&there, r"Far:\n +far\n");
    let r = t.rant(&["ls", "services", "--json"]).ok();
    let services: serde_json::Value = serde_json::from_str(&r.out).expect("ls services --json prints JSON");
    assert_eq!(services[0]["state"], "running");
    assert_eq!(services[1]["start"], "login");
    assert_eq!(services[1]["params"][0], "seat=left");

    // A stop or restart that names nothing leaves the services of an enabled workspace running.
    write(&t.path("Other.run"), "node \"demo/sensor\" {\n  name = \"other\"\n}\n");
    t.rant(&["run", "Other", "--watch", "0"]).ok();
    let r = t.rant(&["restart", "-y", "--no-build", "--watch", "0"]).ok();
    expect_match(&r.out, r"roots: group Other\n");
    expect_match(&r.err, "services keep running, name one to restart it: Plain, Desk seat=left");
    expect_match(&r.out, "other +restarted");
    expect_no_match(&r.out, "steady|desk");
    let r = t.rant(&["stop", "-y"]).ok();
    expect_match(&r.out, r"nodes: +other\n");
    expect_match(&r.err, "services keep running, name one to stop it: Plain, Desk seat=left");
    expect_match(&r.out, "other +stopped");
    let r = t.rant(&["stop", "-y"]).ok();
    expect_match(&r.out, "only services run here, name one to stop it: Plain, Desk seat=left");
    let r = t.rant(&["stop", "--all", "-y"]).ok();
    expect_match(&r.out, "only services run on this machine, name one to stop it: Plain, Desk seat=left, Far");

    // disable with a group removes its block and stops its run, keeping the rest of the file, and
    // a group another workspace serves is pointed there.
    let r = t.rant(&["disable", "Desk"]).ok();
    expect_match(&r.out, "desk_left +stopped");
    expect_match(&r.err, "removed Desk seat=left from the services in workspace.rant");
    expect_match(&read(&t.path("workspace.rant")), r#"^service "Plain" \{\}\n\n# where the operator sits\n$"#);
    let r = t.rant(&["disable", "Desk"]).fails();
    expect_match(&r.err, "`Desk` is not a service here");
    let r = t.rant(&["disable", "Far"]).fails();
    expect_match(&r.err, r"`Far` is a service of [^\n]*-other, disable it there");

    // A bare disable stops the services and drops the start, and the blocks stay.
    let r = t.rant(&["disable"]).ok();
    expect_match(&r.out, "steady +stopped");
    expect_match(&r.err, "disabled this workspace, its services no longer start when you log in");
    expect_match(&read(&t.path("workspace.rant")), r#"service "Plain""#);
    let r = t.rant(&["disable"]).fails();
    expect_match(&r.err, "this workspace is not enabled");
    let r = t.rant(&["ls", "services"]).ok();
    expect_match(&r.out, r"SERVICES\n  Far +at boot +running[^\n]*\n  Plain +not enabled +stopped");
    let r = t.rant(&["disable"]).cwd(&t.other).ok();
    expect_match(&r.out, "far +stopped");
    let r = t.rant(&["ls", "services"]).ok();
    expect_match(&r.out, r"SERVICES\n  Plain +not enabled +stopped[^\n]*\n$");
    t.wait_for_service_log(r"stopped, nothing to watch\n$");
}
