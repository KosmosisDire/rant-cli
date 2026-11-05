mod common;
use common::*;

const NODE: &[&str] = &["--name", "talker", "--pub", "chatter", "--var", "speed", "--fn", "double", "--task", "count"];

#[test]
fn mesh_read() {
    let t = Test::new("mesh_read", 141);

    // An empty mesh lists nothing, and says so.
    let r = t.rant(&["ls", "nodes"]).ok();
    expect_match(&r.out, r"NODES\n  [(]none[)]");

    let r = t.beside_node(NODE, &["ls", "nodes"]);
    expect_match(&r.out, r"NODES\n  talker\n");
    let r = t.beside_node(NODE, &["ls", "entities"]);
    expect_match(&r.out, "chatter +topic");
    expect_match(&r.out, "speed +var");
    expect_match(&r.out, "double +fn");
    expect_match(&r.out, "count +task");

    let r = t.beside_node(NODE, &["ls", "entities", "cha", "--json"]);
    expect_match(&r.out, r#""name": "chatter""#);
    expect_no_match(&r.out, r#"speed|"nodes""#);

    // Piped, sub prints a record per message in any format: the sender, the rate and the value.
    let r = t.beside_node(NODE, &["sub", "chatter", "--count", "3"]);
    expect_match(&r.out, r"^\{from: talker, hz: +[0-9]+[.][0-9][0-9], value: \{seq: +[0-9]+, value: +[0-9]+[.][0-9][0-9]\}\}\n\{from: ");

    let r = t.beside_node(NODE, &["sub", "chatter", "--count", "2", "--csv"]);
    expect_match(&r.out, r"^from,hz,value[.]seq,value[.]value\ntalker,[0-9.]+,[0-9]+,[0-9.]+\ntalker,[0-9.]+,[0-9]+,[0-9.]+\n$");

    let r = t.beside_node(NODE, &["sub", "chatter", "--count", "1", "--json"]);
    expect_match(&r.out, r#"^\{"from":"talker","hz":[0-9.]+,"value":\{"seq":[0-9]+,"value":[0-9.]+\}\}\n$"#);

    // --value-only leaves the sender and rate out.
    let r = t.beside_node(NODE, &["sub", "chatter", "--count", "2", "--value-only", "--json"]);
    expect_match(&r.out, r#"^\{"seq":[0-9]+,"value":[0-9.]+\}\n\{"seq":[0-9]+,"value":[0-9.]+\}\n$"#);

    let r = t.beside_node(NODE, &["sub", "chatter", "--count", "1", "--value-only", "--csv"]);
    expect_match(&r.out, r"^seq,value\n[0-9]+,[0-9.]+\n$");

    let r = t.beside_node(NODE, &["info", "talker"]);
    expect_match(&r.out, r"^node: +talker\n");
    expect_match(&r.out, "pid: +[0-9]+");
    expect_match(&r.out, r"publishes: +\[chatter\]");

    let r = t.beside_node(NODE, &["info", "speed"]);
    expect_match(&r.out, r"^var: +speed\n");
    expect_match(&r.out, "type: +f64");
    expect_match(&r.out, r"owner: +\[talker\]");
    expect_match(&r.out, "value: +1.50");

    let r = t.beside_node(NODE, &["info", "chatter"]);
    expect_match(&r.out, r"type: +[|]\n  Reading \{\n    seq: +u32\n    value: f64\n  \}\n");

    let r = t.rant(&["info", "nothing_here"]).fails();
    expect_match(&r.err, "nothing named");
}
