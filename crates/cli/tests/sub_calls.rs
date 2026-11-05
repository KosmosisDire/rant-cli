mod common;
use common::*;

const NODE: &[&str] = &["--name", "writer", "--fn", "double", "--task", "count"];
const TIME: &str = "[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[.][0-9]{3}Z";

#[test]
fn sub_calls() {
    let t = Test::new("sub_calls", 162);

    // sub on a function prints a line per step of each call another node makes, and -n ends it
    // after that many calls ended.
    let r = t.while_calling(NODE, &["sub", "double", "-n", "1"], &[&["call", "double", "21"]]);
    let head = format!("time: {TIME}, from: writer, caller: \"@rant/cli\", call: +[0-9]+");
    expect_match(&r.out, &format!(r"^\{{event: call, {head}, value: +21[.]00\}}\n\{{event: end, {head}, status: ok, took: +[0-9.]+, value: +42[.]00\}}\n$"));
    expect_no_match(&r.err, "waiting");

    // A task gives its start and each update before its end.
    let r = t.while_calling(NODE, &["sub", "count", "-n", "1"], &[&["call", "count", "2"]]);
    let lines = [
        format!(r"\{{event: call, {head}, value: +2\}}"),
        format!(r"\{{event: running, {head}\}}"),
        format!(r"\{{event: progress, {head}, value: +1\}}"),
        format!(r"\{{event: progress, {head}, value: +2\}}"),
        format!(r"\{{event: end, {head}, status: ok, took: +[0-9.]+, value: +2\}}"),
    ];
    expect_match(&r.out, &format!("^{}\n$", lines.join("\n")));

    // A cancel names the node that asked, and the end says cancelled.
    let r = t.while_calling(NODE, &["sub", "count", "-n", "1"], &[&["node", "--name", "canceller", "--cancel-call", "count"]]);
    expect_match(&r.out, &format!(r"\{{event: cancel, time: {TIME}, from: writer, caller: canceller, call: +[0-9]+, by: canceller\}}\n"));
    expect_match(&r.out, r"\{event: end, [^}]*caller: canceller, [^}]*status: cancelled, took: +[0-9.]+, value: +[0-9]+\}\n$");

    // JSON lines, CSV with every column, and the values alone.
    let r = t.while_calling(NODE, &["sub", "double", "-n", "1", "--json"], &[&["call", "double", "21"]]);
    let head = format!(r#""time":"{TIME}","from":"writer","caller":"@rant/cli","call":[0-9]+"#);
    expect_match(&r.out, &format!(r#"^\{{"event":"call",{head},"value":21[.]0\}}\n\{{"event":"end",{head},"status":"ok","took":[0-9.e-]+,"value":42[.]0\}}\n$"#));

    let r = t.while_calling(NODE, &["sub", "double", "-n", "1", "--csv"], &[&["call", "double", "21"]]);
    let head = format!("{TIME},writer,@rant/cli,[0-9]+");
    expect_match(&r.out, &format!(r"^event,time,from,caller,call,status,by,message,took,value\ncall,{head},,,,,21[.]0\nend,{head},ok,,,[0-9.e-]+,42[.]0\n$"));

    let r = t.while_calling(NODE, &["sub", "double", "-n", "1", "--value-only"], &[&["call", "double", "21"]]);
    expect_match(&r.out, r"^ *21[.]00\n *42[.]00\n$");
}
