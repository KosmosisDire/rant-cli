mod common;
use common::*;

const NODE: &[&str] = &["--name", "writer", "--pub", "chatter", "--var", "speed", "--fn", "double", "--task", "count"];

#[test]
fn mesh_write() {
    let t = Test::new("mesh_write", 142);

    // Every entity kind is read and written from the shell, in the types the mesh declares. sub
    // and pub take a variable as a topic: its value at once from its owner, and a write the owner applied.
    let r = t.beside_node(NODE, &["sub", "speed", "--count", "1"]);
    expect_match(&r.out, r"^\{from: writer, hz: +0[.]00, value: +1[.]50\}\n$");

    let r = t.beside_node(NODE, &["sub", "speed", "--count", "1", "--json"]);
    expect_match(&r.out, r#"^\{"from":"writer","hz":0[.]0,"value":1[.]5\}\n$"#);

    let r = t.beside_node(NODE, &["sub", "speed", "--count", "1", "--csv"]);
    expect_match(&r.out, r"^from,hz,value\nwriter,0[.]0,1[.]5\n$");

    let r = t.beside_node(NODE, &["pub", "speed", "3.25"]);
    expect_match(&r.err, "wrote 1 value to speed");

    let r = t.beside_node(NODE, &["call", "double", "21"]);
    expect_match(&r.out, r"^42.00\n$");

    // A task's progress is a stream as sub prints it, and the result is the last line.
    let r = t.beside_node(NODE, &["call", "count", "3"]);
    expect_match(&r.out, r"^\{from: writer, hz: +[0-9.]+, value: +1\}\n\{from: writer, hz: +[0-9.]+, value: +2\}\n\{from: writer, hz: +[0-9.]+, value: +3\}\n3\n$");
    expect_match(&r.err, "task [0-9]+ running, Ctrl-C cancels it");

    let r = t.beside_node(NODE, &["call", "count", "2", "--json"]);
    expect_match(&r.out, r#"^\{"from":"writer","hz":[0-9.]+,"value":1\}\n\{"from":"writer","hz":[0-9.]+,"value":2\}\n2\n$"#);

    let r = t.beside_node(NODE, &["call", "count", "2", "--csv"]);
    expect_match(&r.out, r"^value\n2\n$");

    let r = t.beside_node(NODE, &["call", "count", "2", "--value-only", "--json"]);
    expect_match(&r.out, r"^1\n2\n2\n$");

    let r = t.beside_node(NODE, &["pub", "chatter", "seq=7", "value=0.5"]);
    expect_match(&r.err, "published 1 message to chatter");

    let r = t.beside_node(NODE, &["pub", "chatter", "{seq: 8, value: 1}", "--rate", "20", "--count", "3"]);
    expect_match(&r.err, "published 3 messages to chatter");

    // Mistakes name the entity, the field or the value.
    let r = t.rant(&["pub", "speed", "1"]).fails();
    expect_match(&r.err, "no topic or var named `speed` is on the mesh");
    let r = t.rant(&["pub", "speed"]).fails();
    expect_match(&r.err, "pub takes a topic or variable and a value");
    let r = t.rant(&["pub", "chatter", "{seq: 1"]).fails();
    expect_match(&r.err, "cannot read the value");
}
