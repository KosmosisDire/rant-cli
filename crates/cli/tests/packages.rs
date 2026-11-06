mod common;
use common::*;

#[test]
fn packages() {
    let t = Test::new("packages", 159);
    let home = &t.rant_home;

    // A package as its repo builds it: package.rant, a group and the program in bin/. Nothing here
    // reaches the network: packages come from this folder, linked, or from an archive of it.
    let build = t.path("tool-build");
    t.install_test_node("tool-build/bin", "tool");
    write(&format!("{build}/package.rant"), "name = \"tool\"\ndescription = \"A test tool\"\nnode \"worker\" {\n  run = [\"${path.package}/bin/tool\", \"--life\", \"60\"]\n}\n");
    write(&format!("{build}/Tool.run"), "node \"tool/worker\" {}\n");
    tar(&build, "czf", &t.path("tool.tar.gz"), &["package.rant", "Tool.run", "bin"], None);

    // Usage mistakes.
    let r = t.rant(&["install"]).fails();
    expect_match(&r.err, "install needs a package");
    let r = t.rant(&["install", "tool", "other", "--from", "tool.tar.gz"]).fails();
    expect_match(&r.err, "--from takes one package");
    let r = t.rant(&["install", "tool", "--from", "tool.tar.gz", "--version", "1.0"]).fails();
    expect_match(&r.err, "--version picks a release, and --from has none");
    let r = t.rant(&["install", "Tool", "--from", "tool.tar.gz"]).fails();
    expect_match(&r.err, "`Tool` is no package name, which is lowercase letters, digits and _, and no GitHub repo");
    let r = t.rant(&["install", "someone/tool", "--from", "tool.tar.gz"]).fails();
    expect_match(&r.err, "--from takes the name of the package it holds, not a repo");
    let r = t.rant(&["remove", "tool"]).fails();
    expect_match(&r.err, "tool is not installed");
    let r = t.rant(&["install", "tool", "--from", "missing"]).fails();
    expect_match(&r.err, r"no folder or archive [^\n]*missing for tool");
    write(&t.path("empty/readme.md"), "nothing\n");
    tar(&t.path("empty"), "czf", &t.path("empty.tar.gz"), &["readme.md"], None);
    let r = t.rant(&["install", "tool", "--from", "empty.tar.gz"]).fails();
    expect_match(&r.err, "holds no package[.]rant at its top");
    assert!(!exists(&format!("{home}/packages/tool")), "a refused archive was installed");

    // A package's own package.rant names it, so another name is refused.
    let r = t.rant(&["install", "other", "--from", "tool-build"]).fails();
    expect_match(&r.err, "tool-build holds the package tool, not other");
    let r = t.rant(&["install", "other", "--from", "tool.tar.gz"]).fails();
    expect_match(&r.err, "tool[.]tar[.]gz holds the package tool, not other");
    let left = list(&format!("{home}/packages"));
    assert!(left.is_empty(), "a refused name left {left:?}");

    // An archive unpacks into rant's home, and its nodes and groups count everywhere.
    let r = t.rant(&["install", "tool", "--from", "tool.tar.gz"]).ok();
    expect_match(&r.err, r"installed tool from [^\n]*tool[.]tar[.]gz");
    expect_match(&read(&format!("{home}/packages/tool.json")), r#""named_by": "--from""#);
    let r = t.rant(&["ls", "packages"]).ok();
    expect_match(&r.out, r"\n  tool +package[.]rant +installed");
    let r = t.rant(&["run", "Tool"]).ok();
    expect_match(&r.out, "worker +started");

    // While its node runs it can neither go nor change.
    let r = t.rant(&["remove", "tool"]).fails();
    expect_match(&r.err, "worker runs from tool, stop it first with `rant stop`");
    let r = t.rant(&["install", "tool", "--from", "tool.tar.gz"]).fails();
    expect_match(&r.err, "worker runs from tool, stop it first");
    t.rant(&["stop", "Tool"]).ok();
    t.rant(&["install", "tool", "--from", "tool.tar.gz"]).ok();
    let r = t.rant(&["remove", "tool"]).ok();
    expect_match(&r.err, "removed tool");
    let left = list(&format!("{home}/packages"));
    assert!(left.is_empty(), "remove left {left:?}");

    // sources.rant lists local build folders, relative to itself, which are linked, never copied,
    // each named by its package.rant.
    write(&t.path("nameless/package.rant"), "description = \"No name\"\n");
    write(&format!("{home}/sources.rant"), "paths     = [\"../../tool-build\", \"../../nameless\"]\ndatabases = [\"someone/packages\"]\n");
    let r = t.rant(&["ls", "sources"]).ok();
    expect_match(&r.out, r"^SOURCES\n  [^\n]*/tool-build +path *\n  [^\n]*/nameless +path *\n  KosmosisDire/rant-packages +database +official\n  someone/packages +database *\n");
    let r = t.rant(&["install", "tool"]).ok();
    expect_match(&r.err, r"linked tool to [^\n]*tool-build");
    let r = t.rant(&["install", "nameless", "--from", "nameless"]).fails();
    expect_match(&r.err, r"the package[.]rant of [^\n]*nameless gives no name");
    let r = t.rant(&["info", "package", "tool"]).ok();
    expect_match(&r.out, r"\ndescription: A test tool\n");
    write(&format!("{build}/Later.run"), "node \"tool/worker\" {}\n");
    let r = t.rant(&["ls", "groups"]).ok();
    expect_match(&r.out, "tool/Later");
    t.rant(&["remove", "tool"]).ok();
    assert!(exists(&format!("{build}/package.rant")) && exists(&format!("{build}/bin")), "removing a linked package reached into its folder");

    // Update finds a linked folder current by itself, and follows sources.rant to an archive,
    // then takes the archive again once it changes.
    write(&format!("{home}/sources.rant"), "paths = [\"../../tool-build\"]\n");
    t.rant(&["install", "tool"]).ok();
    let r = t.rant(&["update", "tool"]).ok();
    expect_match(&r.out, r"^tool is up to date\n$");
    write(&format!("{home}/sources.rant"), "paths = [\"../../tool.tar.gz\"]\n");
    let r = t.rant(&["update", "tool", "-y"]).ok();
    expect_match(&r.out, r"This updates:\n  tool, from [^\n]*tool[.]tar[.]gz\n");
    expect_match(&r.err, "installed tool from");
    let r = t.rant(&["update", "tool"]).ok();
    expect_match(&r.out, r"^tool is up to date\n$");
    sleep(1.1);
    touch(&t.path("tool.tar.gz"));
    let r = t.rant(&["update", "tool", "-y"]).ok();
    expect_match(&r.err, "installed tool from");

    // An update stops the nodes running from the package and starts them again after.
    t.rant(&["run", "Tool"]).ok();
    sleep(1.1);
    touch(&t.path("tool.tar.gz"));
    let r = t.rant(&["update", "tool", "-y"]).ok();
    expect_match(&r.err, "stopped worker while tool changes.*installed tool from.*started worker again");
    let r = t.rant(&["ls", "nodes", "--no-mesh"]).ok();
    expect_match(&r.out, r"Tool:\n +worker\n");
    t.rant(&["stop", "Tool"]).ok();

    // So does one the service process runs, which it starts again.
    write(&format!("{build}/Kept.run"), "restart = \"always\"\nnode \"tool/worker\" {}\n");
    tar(&build, "czf", &t.path("tool.tar.gz"), &["package.rant", "Tool.run", "Kept.run", "bin"], None);
    t.rant(&["update", "tool", "-y"]).ok();
    t.rant(&["run", "Kept", "--watch", "0"]).ok();
    sleep(1.1);
    touch(&t.path("tool.tar.gz"));
    let r = t.rant(&["update", "tool", "-y"]).ok();
    expect_match(&r.err, "stopped worker while tool changes.*installed tool from.*started worker again");
    let r = t.rant(&["ls", "nodes", "--no-mesh"]).ok();
    expect_match(&r.out, r"Kept:\n +worker\n");
    t.rant(&["stop", "Kept"]).ok();
    t.wait_for_service_log(r"stopped, nothing to watch\n$");

    // What --from installed stays until rant install says otherwise.
    t.rant(&["install", "tool", "--from", "tool-build"]).ok();
    let r = t.rant(&["update", "tool"]).ok();
    expect_match(&r.err, r"tool came from [^\n]*tool-build through --from, so update leaves it alone");
    let r = t.rant(&["update", "other"]).fails();
    expect_match(&r.err, "other is not installed, `rant install other` installs it");
    t.rant(&["remove", "tool"]).ok();

    // A package a repo's link installed is never replaced from elsewhere, nor the other way.
    mkdir(&format!("{home}/packages/tool"));
    write(&format!("{home}/packages/tool.json"), "{ \"repo\": \"someone/tool\", \"named_by\": \"link\", \"version\": \"1.0\" }\n");
    let r = t.rant(&["install", "tool", "--from", "tool-build"]).fails();
    expect_match(&r.err, r"tool is installed from someone/tool, `rant remove tool` first to install it from [^\n]*tool-build");
    t.rant(&["remove", "tool"]).ok();

    // A broken sources.rant says where.
    write(&format!("{home}/sources.rant"), "paths = [\"\"]\n");
    let r = t.rant(&["install", "tool"]).fails();
    expect_match(&r.err, "sources[.]rant:1:[0-9]+: a path must name a folder or an archive");
    let r = t.rant(&["ls", "sources"]).fails();
    expect_match(&r.err, "a path must name a folder or an archive");

    // A package's skill folder is linked for each AI coding agent found, and goes with it. A
    // package that is more than a skill asks first, until its skill is given.
    remove(&format!("{home}/sources.rant"));
    write(&format!("{build}/skill/SKILL.md"), "---\nname: tool\ndescription: Use the tool\n---\n");
    let r = t.rant(&["install", "tool", "--from", "tool-build"]).ok();
    expect_match(&r.err, "found no AI coding agent here to give the skill of tool to");
    mkdir(&format!("{}/.claude", t.home));
    mkdir(&format!("{}/.cursor", t.home));
    let r = t.rant(&["install", "tool", "--from", "tool-build"]).ok();
    expect_match(&r.err, r"Also give the skill of tool to Claude Code, Cursor[?] \[y/N\] y");
    expect_match(&r.err, r"linked the skill of tool for Claude Code as [^\n]*[.]claude.skills.tool\n");
    expect_match(&r.err, r"linked the skill of tool for Cursor as [^\n]*[.]agents.skills.tool\n");
    expect_match(&read(&format!("{}/.claude/skills/tool/SKILL.md", t.home)), r"^---\nname: tool\n");
    expect_match(&read(&format!("{home}/packages/tool.json")), r#""skills": \["#);
    let r = t.rant(&["install", "tool", "--from", "tool-build"]).ok();
    expect_no_match(&r.err, "linked the skill of tool|Also give");
    t.rant(&["remove", "tool"]).ok();
    for gone in [format!("{}/.claude/skills/tool", t.home), format!("{}/.agents/skills/tool", t.home)] {
        assert!(!exists(&gone), "remove left {gone}");
    }
    assert!(exists(&format!("{build}/skill/SKILL.md")), "remove reached into the skill's folder");

    // A package that is only a skill gives it without asking, linked with each _ as -.
    write(&t.path("lore/package.rant"), "name = \"lore_skill\"\n");
    write(&t.path("lore/skill/SKILL.md"), "---\nname: lore-skill\ndescription: Lore\n---\n");
    let r = t.rant(&["install", "lore_skill", "--from", "lore"]).ok();
    expect_no_match(&r.err, "Also give");
    expect_match(&r.err, r"linked the skill of lore_skill for Claude Code as [^\n]*[.]claude.skills.lore-skill\n");
    t.rant(&["remove", "lore_skill"]).ok();

    // remove takes the skill rant add gave a library as well.
    write(&format!("{home}/skills/geometry/SKILL.md"), "---\nname: geometry\n---\n");
    write(&format!("{home}/skills/geometry.json"), "{\"repo\": \"someone/geometry\", \"version\": \"1.0.0\", \"links\": []}\n");
    let r = t.rant(&["remove", "geometry"]).ok();
    expect_match(&r.err, "removed the skill of geometry");
    for gone in [format!("{home}/skills/geometry"), format!("{home}/skills/geometry.json")] {
        assert!(!exists(&gone), "remove left {gone}");
    }

    // Uninstall drops a linked package without reaching into its folder.
    let r = t.rant(&["install", "tool", "--from", "tool-build"]).ok();
    expect_match(&r.err, "linked tool to");
    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.out, r"uninstalled rant-cli\n");
    assert!(!exists(home), "uninstall left {home}");
    assert!(exists(&format!("{build}/package.rant")) && exists(&format!("{build}/bin")), "uninstall reached into a linked package's folder");
}
