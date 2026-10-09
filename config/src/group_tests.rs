//! Group resolution against a small workspace: expansion order, templating, includes with
//! bound and exposed params, merging, and every semantic error with its place.

use std::collections::BTreeMap;

use hcl::Value;

use crate::diag::Diag;
use crate::group::Groups;
use crate::param::parse_value;
use crate::model::Model;
use crate::plan::Plan;
use crate::testdir::TestDir;
use crate::workspace;

fn workspace_with(extra: &[(&str, &str)]) -> TestDir {
    let t = TestDir::new();
    t.write("rant.hcl", "workspace {}\n");
    t.write(
        "drivers/rant.hcl",
        "package {\n  name = \"drivers\"\n  node \"lidar\" {\n    run = \"lidar --fast\"\n  }\n  node \"camera\" {\n    run = \"cam\"\n  }\n  node \"motors\" {\n    run = \"motors\"\n  }\n}\n",
    );
    t.write(
        "planner/rant.hcl",
        "package {\n  name = \"planner\"\n  node \"planner\" {\n    run = \"plan\"\n  }\n  node \"odom\" {\n    run = \"odom\"\n  }\n}\n",
    );
    t.write(
        "drivers/launch/cameras.group.hcl",
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
        "base.group.hcl",
        "  param \"lidar_args\" {\n    default = \"--range 30\"\n  }\n  node \"drivers/lidar\" {\n    args = param.lidar_args\n  }\n  node \"planner/odom\" {}\n",
    );
    t.write(
        "nav.group.hcl",
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
    t.write("pick.group.hcl", "  include \"base\" {}\n  node \"drivers/motors\" {\n    name = \"arm\"\n  }\n");
    for (path, text) in extra {
        t.write(path, text);
    }
    t
}

fn model(t: &TestDir) -> Model {
    let (m, diags) = Model::load(workspace::open(t.root()).unwrap().unwrap());
    assert!(diags.is_empty(), "{diags:?}");
    m
}

fn plan(m: &Model, group: &str, params: &[(&str, &str)]) -> Result<(Plan, BTreeMap<String, Value>), Diag> {
    let groups = Groups::new(m);
    let file = groups.find(group, None).map_err(Diag::plain)?;
    let def = groups.load(file)?;
    let iface = groups.interface(&def)?;
    let mut given = BTreeMap::new();
    for (k, v) in params {
        let p = iface.iter().find(|p| p.name == *k).expect("a known param");
        given.insert(k.to_string(), parse_value(p, v).map_err(Diag::plain)?);
    }
    groups.plan(&def, given)
}

fn names(p: &Plan) -> Vec<&str> {
    p.instances.iter().map(|i| i.name.as_str()).collect()
}

#[test]
fn groups_are_named_by_package_and_stem() {
    let t = workspace_with(&[]);
    let m = model(&t);
    let names: Vec<&str> = m.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["base", "drivers/cameras", "nav", "pick"]);
}

#[test]
fn a_group_expands_depth_first_in_written_order() {
    let t = workspace_with(&[]);
    let (p, values) = plan(&model(&t), "nav", &[("speed", "2.5")]).unwrap();
    assert_eq!(names(&p), ["lidar", "odom", "camera_left", "camera_right", "planner"]);
    assert_eq!(values["speed"], Value::from(2.5));
    assert_eq!(values["lidar_args"], Value::from("--range 30"), "exposed from base");

    let left = &p.instances[2];
    assert_eq!(left.argv, ["cam", "--fps", "30", "--label", "left side"]);
    assert_eq!(left.env["SIDE"], "left");
    assert_eq!(p.instances[3].argv[2], "60");
    assert_eq!(p.instances[4].argv, ["plan", "--speed", "2.5"]);
    assert_eq!(p.instances[0].argv, ["lidar", "--fast", "--range", "30"]);
}

#[test]
fn exposed_params_reach_the_included_group() {
    let t = workspace_with(&[]);
    let (p, _) = plan(&model(&t), "nav", &[("lidar_args", "--range 5")]).unwrap();
    assert_eq!(p.instances[0].argv, ["lidar", "--fast", "--range", "5"]);
}

#[test]
fn values_are_checked_before_anything_runs() {
    let t = workspace_with(&[]);
    let m = model(&t);
    assert!(plan(&m, "drivers/cameras", &[]).unwrap_err().message.contains("param `side` has no default"));
    let groups = Groups::new(&m);
    let def = groups.load(groups.find("drivers/cameras", None).unwrap()).unwrap();
    let iface = groups.interface(&def).unwrap();
    let side = iface.iter().find(|p| p.name == "side").unwrap();
    assert!(parse_value(side, "up").unwrap_err().contains("must be one of left, right"));
    let fps = iface.iter().find(|p| p.name == "fps").unwrap();
    assert!(parse_value(fps, "fast").unwrap_err().contains("takes an int"));
}

#[test]
fn a_bare_reference_looks_in_its_own_package_first() {
    let t = workspace_with(&[("drivers/launch/front.group.hcl", "  include \"cameras\" {\n    side = \"left\"\n  }\n")]);
    let (p, _) = plan(&model(&t), "drivers/front", &[]).unwrap();
    assert_eq!(names(&p), ["camera_left"]);
    let groups_model = model(&t);
    let groups = Groups::new(&groups_model);
    assert!(groups.find("cameras", None).unwrap_err().contains("did you mean drivers/cameras"));
}

#[test]
fn the_same_node_twice_merges_and_a_different_one_is_an_error() {
    let t = workspace_with(&[
        ("both.group.hcl", "  include \"nav\" {}\n  include \"pick\" {}\n"),
        ("clash.group.hcl", "  include \"base\" {}\n  node \"planner/odom\" {\n    args = \"--other\"\n  }\n"),
    ]);
    let m = model(&t);
    let (p, _) = plan(&m, "both", &[]).unwrap();
    assert_eq!(names(&p), ["lidar", "odom", "camera_left", "camera_right", "planner", "arm"]);
    let err = plan(&m, "clash", &[]).unwrap_err();
    assert!(err.message.contains("node `odom` is also defined at"), "{err}");
    assert!(err.message.contains("base.group.hcl:7:3"), "{err}");
    assert_eq!((err.line, err.column), (2, 3));
}

#[test]
fn a_node_label_names_what_runs() {
    let t = workspace_with(&[("bare.group.hcl", "  node \"lidar\" {}\n  node \"odom\" {\n    args = \"--slow\"\n  }\n")]);
    let m = model(&t);
    let (p, _) = plan(&m, "bare", &[]).unwrap();
    assert_eq!(names(&p), ["lidar", "odom"]);
    assert_eq!(p.instances[0].node.package, "drivers");
    assert_eq!(p.instances[1].node.package, "planner");
}

#[test]
fn semantic_errors_point_at_their_place() {
    let cases: &[(&str, &str, &str, u32)] = &[
        ("undeclared.group.hcl", "  node \"drivers/lidar\" {\n    args = [param.nope]\n  }\n", "no param `nope` is declared", 2),
        ("unbound.group.hcl", "  include \"drivers/cameras\" {}\n", "param `side` has no default, so it must be given, bind it here or expose it", 1),
        ("unknown.group.hcl", "  include \"base\" {\n    colour = \"red\"\n  }\n", "group `base` has no param `colour`", 2),
        ("missing.group.hcl", "  include \"nowhere\" {}\n", "no group named `nowhere`", 1),
        ("badtype.group.hcl", "  node \"drivers/nothing\" {}\n", "has no node `nothing`", 1),
        (
            "collide.group.hcl",
            "  param \"lidar_args\" {}\n  include \"base\" {\n    expose = true\n  }\n",
            "exposed param `lidar_args` collides",
            2,
        ),
        ("cycle_a.group.hcl", "  include \"cycle_b\" {}\n", "include cycle", 0),
        ("nodetype.group.hcl", "  node \"x\" {}\n", "no node named `x`", 1),
        ("default.group.hcl", "  param \"n\" {\n    type    = int\n    default = \"ten\"\n  }\n", "default: param `n` is int", 3),
    ];
    let t = workspace_with(&[("cycle_b.group.hcl", "  include \"cycle_a\" {}\n")]);
    for (file, text, _, _) in cases {
        t.write(file, text);
    }
    let m = model(&t);
    for (file, _, message, line) in cases {
        let group = file.trim_end_matches(".group.hcl");
        let err = plan(&m, group, &[]).unwrap_err();
        assert!(err.message.contains(message), "{group}: {err}");
        assert_eq!(err.line, *line, "{group}: {err}");
    }
}

#[test]
fn a_block_a_group_file_does_not_know_is_an_error() {
    let t = workspace_with(&[("two.group.hcl", "group {}\n")]);
    let m = model(&t);
    let groups = Groups::new(&m);
    assert!(groups.load(groups.find("two", None).unwrap()).is_err());
}

#[test]
fn placement_follows_workspace_package_groups_and_node() {
    let t = workspace_with(&[
        ("rant.hcl", "workspace {\n  domain = 5\n  prefix = \"plant\"\n}\n"),
        ("arm/rant.hcl", "package {\n  name   = \"arm\"\n  prefix = \"arm\"\n  node \"joint\" {\n    run = \"joint\"\n  }\n}\n"),
        ("side.group.hcl", "  param \"side\" {}\n  prefix      = param.side\n  node_prefix = param.side\n  node \"arm/joint\" {\n    domain = 9\n  }\n  node \"lidar\" {}\n"),
        ("cell.group.hcl", "  node_prefix = \"cell\"\n  include \"side\" {\n    side   = \"left\"\n    domain = 7\n  }\n"),
        ("bad.group.hcl", "  param \"domain\" {}\n"),
        ("long.group.hcl", "  node \"lidar\" {\n    node_prefix = \"a_very_long_cell_name\"\n    name        = \"and_a_long_node\"\n  }\n"),
    ]);
    let m = model(&t);
    let (p, _) = plan(&m, "cell", &[]).unwrap();
    assert_eq!(names(&p), ["cell/left/joint", "cell/left/lidar"]);
    assert_eq!(p.instances[0].env["RANT_PREFIX"], "plant/arm/left");
    assert_eq!(p.instances[0].env["RANT_DOMAIN"], "9", "the node is innermost");
    assert_eq!(p.instances[1].env["RANT_PREFIX"], "plant/left");
    assert_eq!(p.instances[1].env["RANT_DOMAIN"], "7", "the include is inside the workspace");
    assert!(plan(&m, "bad", &[]).unwrap_err().message.contains("that name sets placement"));
    let err = plan(&m, "long", &[]).unwrap_err();
    assert!(err.message.contains("longer than Rant's 32 bytes"), "{err}");
    assert_eq!(err.line, 1);
}
