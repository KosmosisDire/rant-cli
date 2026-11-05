mod common;
use common::*;

#[test]
fn init() {
    let t = Test::new("init", 140);

    let r = t.rant(&["init"]).ok();
    expect_match(&r.out, "created .*workspace.rant");
    expect_match(&read(&t.path("workspace.rant")), "^# logs");

    let r = t.rant(&["init"]).fails();
    expect_match(&r.err, "exists already");
}
