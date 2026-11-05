//! Groups against a small workspace: expansion order, templating, includes with bound and
//! exposed params, merging, services, and every semantic error with its place.

mod common;

use common::{TestDir, load, load_at};
use rant_config::plan::{Named, Plan};
use rant_config::{Diag, GroupPlan, Value, Workspace, paths};

fn workspace_with(extra: &[(&str, &str)]) -> TestDir {
    let t = TestDir::new();
    t.write("workspace.rant", "");
    t.write(
        "drivers/package.rant",
        "name = \"drivers\"\nnode \"lidar\" {\n  run = \"lidar --fast\"\n}\nnode \"camera\" {\n  run = \"cam\"\n}\nnode \"motors\" {\n  run = \"motors\"\n}\n",
    );
    t.write("planner/package.rant", "name = \"planner\"\nnode \"planner\" {\n  run = \"plan\"\n}\nnode \"odom\" {\n  run = \"odom\"\n}\n");
    t.write(
        "drivers/launch/cameras.run",
        r#"# The two cameras of the robot, one per side.
  param "side" {
    type    = string
    options = ["left", "right"]
  }
  param "fps" {
    type    = int
    default = 30
  }
  node "drivers/camera" {
    name = "camera_${param.side}"
    args = "--fps ${param.fps} --label '${param.side} side'"
    env  = { SIDE = param.side }
  }
"#,
    );
    t.write(
        "base.run",
        "  param \"lidar_args\" {\n    default = \"--range 30\"\n  }\n  node \"drivers/lidar\" {\n    args = param.lidar_args\n  }\n  node \"planner/odom\" {}\n",
    );
    t.write(
        "nav.run",
        r#"
  description = "Drive around"
  param "speed" {
    type    = float
    default = 1
  }
  include "base" {
    expose = true
  }
  include "drivers/cameras" {
    side = "left"
  }
  include "drivers/cameras" {
    side = "right"
    fps  = 60
  }
  node "planner/planner" {
    args = ["--speed", param.speed]
  }
"#,
    );
    t.write("pick.run", "  include \"base\" {}\n  node \"drivers/motors\" {\n    name = \"arm\"\n  }\n");
    for (path, text) in extra {
        t.write(path, text);
    }
    t
}

fn plan(ws: &Workspace, group: &str, words: &[&str]) -> Result<GroupPlan, Diag> {
    let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    ws.plan_group(group, &words)
}

fn names(p: &Plan) -> Vec<&str> {
    p.instances.iter().map(|i| i.name.as_str()).collect()
}

#[test]
fn groups_are_named_by_package_and_stem() {
    let t = workspace_with(&[]);
    let ws = load(&t);
    let names: Vec<&str> = ws.model.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["base", "drivers/cameras", "nav", "pick"]);
}

#[test]
fn a_group_expands_depth_first_in_written_order() {
    let t = workspace_with(&[]);
    let p = plan(&load(&t), "nav", &["speed=2.5"]).unwrap();
    assert_eq!(p.group, "nav");
    assert_eq!(names(&p.plan), ["lidar", "odom", "camera_left", "camera_right", "planner"]);
    assert_eq!(p.values["speed"], Value::from(2.5));
    assert_eq!(p.values["lidar_args"], Value::from("--range 30"), "exposed from base");

    let i = &p.plan.instances;
    assert_eq!(i[2].argv, ["cam", "--fps", "30", "--label", "left side"]);
    assert_eq!(i[2].env["SIDE"], "left");
    assert_eq!(i[3].argv[2], "60");
    assert_eq!(i[4].argv, ["plan", "--speed", "2.5"]);
    assert_eq!(i[0].argv, ["lidar", "--fast", "--range", "30"]);
}

#[test]
fn a_group_tells_its_params_and_includes() {
    let t = workspace_with(&[]);
    let ws = load(&t);
    let nav = ws.group("nav").unwrap();
    assert_eq!(nav.description.as_deref(), Some("Drive around"));
    let params: Vec<(&str, &str)> = nav.params.iter().map(|p| (p.name.as_str(), p.ty.name())).collect();
    assert_eq!(params, [("speed", "float"), ("lidar_args", "string")], "its own, then the exposed");
    assert_eq!(nav.includes, ["base", "drivers/cameras", "drivers/cameras"]);
    let side = &ws.group("cameras").unwrap().params[0];
    assert_eq!(side.options, [Value::from("left"), Value::from("right")]);
    assert_eq!(side.default, None);
}

#[test]
fn each_node_knows_its_own_group() {
    let t = workspace_with(&[]);
    let ws = load(&t);
    let p = plan(&ws, "nav", &[]).unwrap();
    let own: Vec<(&str, Option<&str>)> = p.plan.instances.iter().map(|i| (i.name.as_str(), i.group.as_deref())).collect();
    assert_eq!(
        own,
        [
            ("lidar", Some("base")),
            ("odom", Some("base")),
            ("camera_left", Some("drivers/cameras")),
            ("camera_right", Some("drivers/cameras")),
            ("planner", Some("nav")),
        ]
    );
    assert_eq!(ws.plan_node("drivers/lidar").unwrap().instances[0].group, None);
}

#[test]
fn exposed_params_reach_the_included_group() {
    let t = workspace_with(&[]);
    let p = plan(&load(&t), "nav", &["lidar_args=--range 5"]).unwrap();
    assert_eq!(p.plan.instances[0].argv, ["lidar", "--fast", "--range", "5"]);
}

#[test]
fn values_are_checked_before_anything_runs() {
    let t = workspace_with(&[]);
    let ws = load(&t);
    let err = |words: &[&str]| plan(&ws, "drivers/cameras", words).unwrap_err().message;
    assert!(err(&[]).contains("param `side` needs a value"));
    assert!(err(&["side=up"]).contains("must be one of left, right"));
    assert!(err(&["side=left", "fps=fast"]).contains("takes an int"));
    assert_eq!(err(&["fps"]), "params are key=value, not `fps`");
    assert!(err(&["colour=red"]).starts_with("group `drivers/cameras` has no param `colour`"));
    assert_eq!(err(&["side=left", "side=right"]), "param `side` is given twice");
}

#[test]
fn a_path_counts_from_where_it_is_given() {
    let t = workspace_with(&[
        (
            "drivers/launch/viewer.run",
            "  param \"project\" {\n    type = path\n  }\n  param \"logs\" {\n    type    = path\n    default = \"logs\"\n  }\n  node \"drivers/camera\" {\n    args = [param.project, param.logs]\n  }\n",
        ),
        ("drivers/launch/both.run", "  include \"viewer\" {\n    project = \"line\"\n  }\n"),
    ]);
    let ws = load(&t);
    let at = |rel: &str| paths::slash(&paths::normalize(&t.path(rel)));
    // the command line counts from where rant runs, a file from its folder
    let p = plan(&ws, "drivers/viewer", &["project=my/hmi"]).unwrap();
    assert_eq!(p.plan.instances[0].argv[1..], [at("my/hmi"), at("drivers/launch/logs")]);
    let p = plan(&load_at(&t, "drivers"), "viewer", &["project=my/hmi"]).unwrap();
    assert_eq!(p.plan.instances[0].argv[1], at("drivers/my/hmi"));
    let p = plan(&ws, "drivers/both", &[]).unwrap();
    assert_eq!(p.plan.instances[0].argv[1], at("drivers/launch/line"));
    let full = at("elsewhere/hmi");
    let p = plan(&ws, "drivers/viewer", &[&format!("project={full}")]).unwrap();
    assert_eq!(p.plan.instances[0].argv[1], full);
}

#[test]
fn a_bare_reference_looks_in_its_own_package_first() {
    let t = workspace_with(&[("drivers/launch/front.run", "  include \"cameras\" {\n    side = \"left\"\n  }\n")]);
    let ws = load(&t);
    assert_eq!(names(&plan(&ws, "drivers/front", &[]).unwrap().plan), ["camera_left"]);
    assert!(ws.group("planner/cameras").unwrap_err().message.contains("did you mean drivers/cameras"));
}

#[test]
fn a_bare_group_name_finds_the_one_package_that_has_it() {
    let short = |ws: &Workspace, name: &str| ws.group_names().into_iter().find(|g| g.file.name == name).unwrap().short;
    let t = workspace_with(&[]);
    let ws = load(&t);
    assert_eq!(ws.group("cameras").unwrap().file.name, "drivers/cameras");
    assert_eq!(short(&ws, "drivers/cameras"), "cameras");
    assert_eq!(short(&load_at(&t, "drivers"), "nav"), "nav");

    // a second package with the name makes the bare name ambiguous, except inside either
    t.write("planner/cameras.run", "  node \"planner/odom\" {}\n");
    let ws = load(&t);
    let err = ws.group("cameras").unwrap_err().message;
    assert_eq!(err, "`cameras` is a group in several packages, name one: drivers/cameras, planner/cameras");
    assert_eq!(load_at(&t, "planner").group("cameras").unwrap().file.name, "planner/cameras");
    assert_eq!(short(&ws, "drivers/cameras"), "drivers/cameras");
    assert_eq!(short(&load_at(&t, "drivers"), "drivers/cameras"), "cameras");

    // a group outside every package comes before one in a package
    t.write("cameras.run", "  node \"planner/odom\" {}\n");
    assert_eq!(load(&t).group("cameras").unwrap().file.name, "cameras");
}

#[test]
fn the_same_node_twice_merges_and_a_different_one_is_an_error() {
    let t = workspace_with(&[
        ("both.run", "  include \"nav\" {}\n  include \"pick\" {}\n"),
        ("clash.run", "  include \"base\" {}\n  node \"planner/odom\" {\n    args = \"--other\"\n  }\n"),
    ]);
    let ws = load(&t);
    let p = plan(&ws, "both", &[]).unwrap();
    assert_eq!(names(&p.plan), ["lidar", "odom", "camera_left", "camera_right", "planner", "arm"]);
    let err = plan(&ws, "clash", &[]).unwrap_err();
    assert!(err.message.contains("node `odom` is also defined at"), "{err}");
    assert!(err.message.contains("base.run:7:3"), "{err}");
    assert_eq!((err.line, err.column), (2, 3));
}

#[test]
fn a_node_label_names_what_runs() {
    let t = workspace_with(&[("bare.run", "  node \"lidar\" {}\n  node \"odom\" {\n    args = \"--slow\"\n  }\n")]);
    let p = plan(&load(&t), "bare", &[]).unwrap().plan;
    assert_eq!(names(&p), ["lidar", "odom"]);
    assert_eq!(p.instances[0].node.package, "drivers");
    assert_eq!(p.instances[1].node.package, "planner");
}

#[test]
fn semantic_errors_point_at_their_place() {
    let cases: &[(&str, &str, &str, u32)] = &[
        ("undeclared.run", "  node \"drivers/lidar\" {\n    args = [param.nope]\n  }\n", "param `nope` is not declared", 2),
        ("unbound.run", "  include \"drivers/cameras\" {}\n", "param `side` needs a value, bind it here or expose it", 1),
        ("unknown.run", "  include \"base\" {\n    colour = \"red\"\n  }\n", "group `base` has no param `colour`", 2),
        ("missing.run", "  include \"nowhere\" {}\n", "no group named `nowhere`", 1),
        ("badtype.run", "  node \"drivers/nothing\" {}\n", "has no node `nothing`", 1),
        ("collide.run", "  param \"lidar_args\" {}\n  include \"base\" {\n    expose = true\n  }\n", "exposed param `lidar_args` collides", 2),
        ("cycle_a.run", "  include \"cycle_b\" {}\n", "include cycle", 0),
        ("nodetype.run", "  node \"x\" {}\n", "no node named `x`", 1),
        ("default.run", "  param \"n\" {\n    type    = int\n    default = \"ten\"\n  }\n", "default: param `n` is int", 3),
        ("pathoptions.run", "  param \"p\" {\n    type    = path\n    options = [\"a\"]\n  }\n", "a path param takes no options", 3),
    ];
    let t = workspace_with(&[("cycle_b.run", "  include \"cycle_a\" {}\n")]);
    for (file, text, _, _) in cases {
        t.write(file, text);
    }
    let ws = load(&t);
    for (file, _, message, line) in cases {
        let group = file.trim_end_matches(".run");
        let err = plan(&ws, group, &[]).unwrap_err();
        assert!(err.message.contains(message), "{group}: {err}");
        assert_eq!(err.line, *line, "{group}: {err}");
    }
}

#[test]
fn a_group_wins_over_a_node_of_its_name_and_names_match_without_case_or_underscores() {
    let t = workspace_with(&[("odom.run", "  node \"planner/odom\" {}\n"), ("WebView.run", "  node \"drivers/camera\" {}\n")]);
    let ws = load(&t);
    let at = |name: &str| ws.named(name);
    assert_eq!(at("nav"), Named::Group);
    assert_eq!(at("NAV"), Named::Group);
    assert_eq!(at("drivers/cameras"), Named::Group);
    assert_eq!(at("Drivers/Cameras"), Named::Group);
    assert_eq!(at("web_view"), Named::Group);
    assert_eq!(at("lidar"), Named::Node);
    assert_eq!(at("LiDar"), Named::Node);
    assert_eq!(at("planner/odom"), Named::Node);
    assert_eq!(at("Planner/O_dom"), Named::Node);
    assert_eq!(at("odom"), Named::Group, "the group wins, `node odom` reaches the node");
    assert_eq!(at("Odom"), Named::Group);
    assert_eq!(at("nowhere"), Named::Neither("no group or node named `nowhere`, see `rant ls groups` and `rant ls nodes`".into()));
    assert!(matches!(at("drivers/nothing"), Named::Neither(e) if e.contains("has no node `nothing`")));
}

#[test]
fn a_block_a_group_file_does_not_know_is_an_error() {
    let t = workspace_with(&[("two.run", "group {}\n")]);
    assert!(load(&t).group("two").is_err());
}

#[test]
fn placement_follows_workspace_package_groups_and_node() {
    let t = workspace_with(&[
        ("workspace.rant", "domain = 5\nprefix = \"plant\"\n"),
        ("arm/package.rant", "name   = \"arm\"\nprefix = \"arm\"\nnode \"joint\" {\n  run = \"joint\"\n}\n"),
        ("side.run", "  param \"side\" {}\n  prefix      = param.side\n  node_prefix = param.side\n  node \"arm/joint\" {\n    domain = 9\n  }\n  node \"lidar\" {}\n"),
        ("cell.run", "  node_prefix = \"cell\"\n  include \"side\" {\n    side   = \"left\"\n    domain = 7\n  }\n"),
        ("bad.run", "  param \"domain\" {}\n"),
        ("long.run", "  node \"lidar\" {\n    node_prefix = \"a_very_long_cell_name\"\n    name        = \"and_a_long_node\"\n  }\n"),
    ]);
    let ws = load(&t);
    let p = plan(&ws, "cell", &[]).unwrap().plan;
    assert_eq!(names(&p), ["cell/left/joint", "cell/left/lidar"]);
    assert_eq!(p.instances[0].env["RANT_PREFIX"], "plant/arm/left");
    assert_eq!(p.instances[0].env["RANT_DOMAIN"], "9", "the node is innermost");
    assert_eq!(p.instances[1].env["RANT_PREFIX"], "plant/left");
    assert_eq!(p.instances[1].env["RANT_DOMAIN"], "7", "the include is inside the workspace");
    assert!(plan(&ws, "bad", &[]).unwrap_err().message.contains("a param cannot be named"));
    let err = plan(&ws, "long", &[]).unwrap_err();
    assert!(err.message.contains("longer than Rant's 32 bytes"), "{err}");
    assert_eq!(err.line, 1);
}

#[test]
fn restart_comes_from_the_innermost_that_sets_it() {
    let t = workspace_with(&[
        ("inner.run", "  node \"lidar\" {}\n  node \"planner/odom\" {\n    restart = \"never\"\n  }\n"),
        ("outer.run", "  restart = \"on-failure\"\n  include \"inner\" {\n    restart = \"always\"\n  }\n  node \"drivers/motors\" {}\n"),
        ("plain.run", "  node \"lidar\" {}\n"),
        ("wrong.run", "  restart = \"sometimes\"\n  node \"lidar\" {}\n"),
        ("named.run", "  param \"restart\" {}\n"),
    ]);
    let ws = load(&t);
    let p = plan(&ws, "outer", &[]).unwrap().plan;
    let got: Vec<(&str, &str)> = p.instances.iter().map(|i| (i.name.as_str(), i.restart.as_str())).collect();
    assert_eq!(got, [("lidar", "always"), ("odom", "never"), ("motors", "on-failure")]);
    let p = plan(&ws, "plain", &[]).unwrap().plan;
    assert_eq!(p.instances[0].restart.as_str(), "never", "nothing set, never");
    assert!(plan(&ws, "wrong", &[]).unwrap_err().message.contains("`restart` is \"never\", \"on-failure\" or \"always\""));
    assert!(plan(&ws, "named", &[]).unwrap_err().message.contains("a param cannot be named `restart`"));
}

#[test]
fn services_are_checked_added_and_removed() {
    let t = workspace_with(&[("workspace.rant", "service \"nav\" {\n  speed = 2.5\n}\nservice \"drivers/cameras\" {}\n")]);
    let ws = load(&t);
    let services = ws.services();
    let nav = services[0].checked.as_ref().unwrap();
    assert_eq!((nav.full.as_str(), nav.words.as_slice()), ("nav", ["speed=2.5".to_string()].as_slice()));
    assert!(services[1].checked.as_ref().unwrap_err().message.contains("param `side` needs a value"));

    assert!(ws.add_service("cameras", &["side=left".into()]).unwrap());
    let ws = load(&t);
    assert!(!ws.add_service("drivers/cameras", &["side=left".into()]).unwrap(), "the same group and params");
    ws.remove_services(&[1]).unwrap();
    let groups: Vec<String> = load(&t).services().iter().map(|s| s.service.group.clone()).collect();
    assert_eq!(groups, ["nav", "cameras"]);
}
