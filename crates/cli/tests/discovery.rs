mod common;
use common::*;

#[test]
fn discovery() {
    let t = Test::new("discovery", 143);

    // A CMake package with a built node, a Python package and a declared node.
    t.use_fixture("mixed");
    t.install_test_node("cpp/build", "talker");

    let r = t.rant(&["build", "--dry-run"]).ok();
    expect_match(&r.out, r"PACKAGES\n  detector +py\n  lidar_driver +cpp\n  tools +tools\n\n");
    expect_match(&r.out, r"NODES\n  detector/detector +python +py/detector.py\n  lidar_driver/talker +native +cpp/build/talker");
    expect_match(&r.out, r"tools/echoer +declared +cmake -E echo hello\n");
    expect_no_match(&r.out, "helpers");

    // The same from a subdirectory, as JSON with absolute paths.
    let r = t.rant(&["build", "--dry-run", "--json"]).cwd(&t.path("py")).ok();
    expect_match(&r.out, r#""name": "talker""#);
    expect_match(&r.out, r#""path": "[A-Za-z]*:?/[^"]*/cpp/build/talker"#);

    // The second run reads the cache.
    assert!(exists(&t.path(".rant/cache/scan.json")), "no scan cache was written");

    // A malformed package.rant is reported where the mistake is.
    write(&t.path("tools/package.rant"), "name = \n");
    let r = t.rant(&["build", "--dry-run"]).fails();
    expect_match(&r.err, "tools/package.rant:[12]:[0-9]+: syntax error");

    // Outside any workspace the command says how to make one.
    let outside = format!("{}/../outside-{}", t.scratch, t.domain);
    mkdir(&outside);
    let r = t.rant(&["build", "--dry-run"]).cwd(&outside).fails();
    expect_match(&r.err, "rant init");
}
