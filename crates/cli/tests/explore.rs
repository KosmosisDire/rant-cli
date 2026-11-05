mod common;
use common::*;

#[test]
fn explore() {
    let t = Test::new("explore", 152);

    let r = t.rant(&["explore"]).fails();
    expect_match(&r.err, "the explorer is not installed, `rant install explorer` installs it");

    // A stand in explorer as rant install leaves one: the CLI itself, which prints its help for
    // `--domain N` and exits, so nothing stays open and no network is needed.
    let pkg = format!("{}/packages/explorer", t.rant_home);
    mkdir(&format!("{pkg}/bin"));
    copy_file(&rant_cli(), &format!("{pkg}/bin/rant_explorer{}", std::env::consts::EXE_SUFFIX));
    write(&format!("{pkg}/package.rant"), "node \"explorer\" {\n  run = \"${path.package}/bin/rant_explorer\"\n}\n");

    let r = t.rant(&["explore"]).ok();
    expect_match(&r.err, "started .*/home/[.]rant/packages/explorer/bin/rant_explorer");

    // The explorer runs detached, so its output reaches the log a moment later.
    let log = format!("{}/explorer.log", t.rant_home);
    let mut text = String::new();
    for _ in 0..=50 {
        text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("Usage: rant") {
            break;
        }
        sleep(0.1);
    }
    expect_match(&text, "rant: build, run and inspect Rant nodes");

    let r = t.rant(&["explore", "extra"]).fails();
    expect_match(&r.err, "explore takes no arguments");

    // explorer is another name for explore.
    let r = t.rant(&["explorer", "--help"]).ok();
    expect_match(&r.out, "Usage: rant explore");
}
