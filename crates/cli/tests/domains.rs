mod common;
use common::*;

/// The node of a group, as `rant ls nodes --json` on a domain shows it.
fn steady(t: &Test, domain: u16) -> serde_json::Value {
    let r = t.rant(&["ls", "nodes", "--json"]).on_domain(domain).ok();
    let nodes: serde_json::Value = serde_json::from_str(&r.out).expect("ls nodes --json prints JSON");
    nodes["groups"][0]["nodes"][0].clone()
}

#[test]
fn domains() {
    // --domain puts every node a bare run or restart starts on that domain, over the
    // workspace's own, and the domain is not kept with the run.
    let t = Test::new("domains", 163);
    t.use_fixture("groups");
    t.install_test_node("demo/bin", "test_node");
    write(&t.path("workspace.rant"), "domain = 163\nservice \"Plain\" {}\n");
    write(&t.path("Plain.run"), "node \"demo/sensor\" {\n  name = \"steady\"\n}\n");

    let r = t.rant(&["run", "--no-build"]).on_domain(164).ok();
    expect_match(&r.out, "steady +started");
    let s = steady(&t, 164);
    assert_eq!((s["domain"].as_u64(), s["on_mesh"].as_bool()), (Some(164), Some(true)), "{s}");
    assert_eq!(steady(&t, 163)["on_mesh"], false);

    // The same group cannot run on two domains from one workspace, since its node names collide.
    let r = t.rant(&["run", "--no-build"]).fails();
    expect_match(&r.err, "`steady` is already running with other settings for group Plain");
    let r = t.rant(&["run", "--no-build"]).on_domain(165).fails();
    expect_match(&r.err, "`steady` is already running with other settings");

    // A restart plans again, so it moves the nodes to the domain it is given, or to the
    // workspace's when given none.
    let r = t.rant(&["restart", "-y", "--no-build"]).on_domain(165).ok();
    expect_match(&r.out, "steady +restarted");
    assert_eq!(steady(&t, 165)["on_mesh"], true);
    t.rant(&["restart", "-y", "--no-build"]).on_domain(163).ok();
    let s = steady(&t, 163);
    assert_eq!((s["domain"].as_u64(), s["on_mesh"].as_bool()), (Some(163), Some(true)), "{s}");

    // enable keeps the domain for every boot. A dry run shows it and keeps nothing.
    let r = t.rant(&["enable", "--dry-run"]).on_domain(164).ok();
    expect_match(&r.out, "env +RANT_DOMAIN=164");
    assert!(!std::path::Path::new(&format!("{}/services.json", t.rant_home)).exists());

    let r = t.rant(&["stop", "-y"]).ok();
    expect_match(&r.out, "steady +stopped");
}
