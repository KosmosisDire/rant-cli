mod common;
use common::*;

const CELL: &[&str] = &["--name", "cell", "--pub", "reading", "--var", "mode", "--fn", "double"];

#[test]
fn recording() {
    let t = Test::new("recording", 166);

    // record takes every name a pattern matches, a function's calls too, and says what it got.
    let r = t.while_calling(CELL, &["record", "*", "--duration", "5", "-o", "one"], &[&["call", "double", "21"]]);
    expect_match(&r.out, r"^recorded [0-9]+ messages of 3 names in 5[.][0-9] s, [0-9.]+ [KM]?B\none[.]mcap\n$");
    expect_match(&r.err, "recording topic `reading`");

    // info tells each name with its sender and count, and ls finds it in the folder.
    let r = t.rant(&["info", "recording", "one"]).ok();
    expect_match(&r.out, r"name: +double\n +kind: +fn\n +type: +\S*\n +from: +cell\n +messages: +2\n");
    expect_match(&r.out, r"name: +reading\n +kind: +topic\n +type: +Reading\n +from: +cell\n +messages: +[0-9]{2,}\n");
    expect_match(&t.rant(&["ls", "recordings", "--json"]).ok().out, r#""name": "one""#);

    // play refuses names a live node sends, and gives the ways out.
    let r = t.with_node(&["--name", "cell", "--pub", "reading"], || t.rant(&["play", "one"]).fails());
    expect_match(&r.err, r"live nodes on domain 166 send these.*\n  topic reading from cell\n.*--prefix.*--domain.*--ignore");

    // Under a prefix it plays beside them, and a subscriber gets the recorded values from the player.
    let play: &[&str] = &["play", "one", "--prefix", "Replay", "--speed", "4", "--yes"];
    let r = t.while_calling(&["--name", "cell", "--pub", "reading"], &["sub", "Replay/reading", "-n", "3", "--json", "--lines"], &[play]);
    expect_match(&r.out, r#"^(\{"from":"player",[^\n]*"value":\{"seq":[0-9]+,"value":[0-9.]+\}\}\n){3}$"#);

    // On another domain, four times as fast, it takes a quarter of the time, and says calls stay out.
    let r = t.rant(&["play", "one", "--speed", "4"]).on_domain(167).ok();
    expect_match(&r.out, r"^played [0-9]+ messages of 2 names, 5[.][0-9] s of recording in 1[.][0-9] s\n$");
    expect_match(&r.err, "calls are not played yet, so `double` is left out");

    // A file that is no recording, and a start past the end, are refused.
    write(&t.path("other.mcap"), "not a recording");
    expect_match(&t.rant(&["play", "other"]).fails().err, "other[.]mcap is not a Rant recording");
    expect_match(&t.rant(&["play", "one", "--start", "60"]).fails().err, r"the recording is 5[.][0-9] s long");
}
