mod common;
use common::*;

#[test]
fn single_node() {
    let t = Test::new("single_node", 144);
    t.use_fixture("nodes");
    t.install_test_node("demo/bin", "test_node");

    // A node starts detached and shows in ls.
    let r = t.rant(&["run", "node", "chatty"]).ok();
    expect_match(&r.out, r"^  [^ ]+ chatty +started +pid [0-9]+ +logs/chatty[.]log\n\n1 started, all running after 2 s\n$");
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  chatty\n");
    let r = t.rant(&["sub", "chatter", "--count", "2"]).ok();
    expect_match(&r.out, "seq");

    // Starting the same type again gives the copy a free name.
    let r = t.rant(&["run", "node", "chatty"]).ok();
    expect_match(&r.out, "chatty_1 +started");
    let r = t.rant(&["stop", "chatty_1"]).ok();
    expect_match(&r.out, "chatty_1 +stopped");

    // It stops gracefully: the node itself saw the request and said so in its log.
    let r = t.rant(&["stop", "chatty"]).ok();
    expect_match(&r.out, "chatty +stopped");
    let log = read(&t.path("logs/chatty.log"));
    expect_match(&log, "ready");
    expect_match(&log, "stopped");

    // A node that ignores the request is killed after the grace period.
    t.rant(&["run", "node", "stubborn"]).ok();
    let r = t.rant(&["stop", "stubborn"]).ok();
    expect_match(&r.out, "stubborn +killed +did not stop within 3 s");

    // A node that exits within the watch fails the run with the end of its log, and is pruned
    // at the next command.
    let r = t.rant(&["run", "node", "brief"]).fails();
    expect_match(&r.out, r"brief +exited after [0-9.]+ s +logs/brief[.]log\n");
    expect_match(&r.out, r"brief, the end of logs/brief[.]log:\n");
    expect_match(&r.out, r"0 of 1 running, brief exited[.] `rant log brief` shows why\n");
    let r = t.rant(&["ls", "nodes", "--json"]).ok();
    expect_no_match(&r.out, r#""name": "brief""#);
    let r = t.rant(&["stop", "brief"]).fails();
    expect_match(&r.err, "no node `brief` on domain");

    // Mistakes are named.
    let r = t.rant(&["run", "node", "nope"]).fails();
    expect_match(&r.err, "no node named `nope`");
    let r = t.rant(&["run", "chatty"]).fails();
    expect_match(&r.err, "`chatty` is a node, run it in a group or alone with `rant run node chatty`");
    let r = t.rant(&["run"]).fails();
    expect_match(&r.err, "say which group to run");

    expect_match(&read(&t.path(".rant/state")), r#""instances": \[\]"#);

    // A node another workspace started stops from here too.
    mkdir(&t.other);
    copy_into(&t.path("demo"), &t.other);
    write(&format!("{}/workspace.rant", t.other), "");
    t.rant(&["run", "node", "chatty"]).cwd(&t.other).ok();
    let r = t.rant(&["stop", "chatty"]).ok();
    expect_match(&r.out, "chatty +stopped");
    let r = t.rant(&["ls", "nodes", "--json"]).cwd(&t.other).ok();
    expect_no_match(&r.out, r#""name": "chatty""#);

    // So does a node rant did not start, found on the mesh. Windows can only kill it.
    let r = t.beside_node(&["--name", "handmade"], &["stop", "handmade"]);
    expect_match(&r.out, "handmade +(stopped|killed)");

    // --all stops every node on the machine, whichever workspace started it, and so does a
    // bare stop outside any workspace.
    t.rant(&["run", "node", "chatty"]).cwd(&t.other).ok();
    t.rant(&["run", "node", "chatty"]).ok();
    let r = t.rant(&["stop", "--all"]).ok();
    expect_match(&r.err, r"-other: +chatty\n");
    expect_match(&r.err, "Stop all 2 nodes");
    t.rant(&["run", "node", "chatty"]).cwd(&t.other).ok();
    let outside = format!("{}-outside", t.scratch);
    mkdir(&outside);
    let r = t.rant(&["stop"]).cwd(&outside).ok();
    expect_match(&r.err, "Stop all 1 node[?]");
    remove(&outside);
    let r = t.rant(&["stop", "--all"]).ok();
    expect_match(&r.out, "no node is running on this machine");
}
