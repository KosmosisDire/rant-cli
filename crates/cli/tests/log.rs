mod common;
use common::*;

#[test]
fn log() {
    let t = Test::new("log", 155);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");

    t.rant(&["run", "nav"]).ok();

    // One node's log is printed as it is. The test node prints its own default name.
    let r = t.rant(&["log", "lidar"]).ok();
    expect_match(&r.out, r"^ready test_node\n$");
    let r = t.rant(&["log", "lidar", "-n", "0"]).ok();
    expect_match(&r.out, "^$");

    // A group's logs, or every running node's, interleave with each line led by its node.
    let r = t.rant(&["log", "nav"]).ok();
    expect_match(&r.out, r"^lidar   [|] ready test_node\nodom    [|] ready test_node\nplanner [|] ready test_node\n$");
    let r = t.rant(&["log"]).ok();
    expect_match(&r.out, r"lidar   [|] ready test_node\n");

    // The log of a node that stopped stays readable.
    t.rant(&["stop", "nav"]).ok();
    let r = t.rant(&["log", "lidar"]).ok();
    expect_match(&r.out, r"^ready test_node\nstopped\n$");

    let r = t.rant(&["log", "nowhere"]).fails();
    expect_match(&r.err, "`nowhere` has no log");
}
