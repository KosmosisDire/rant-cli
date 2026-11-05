mod common;
use common::*;

#[test]
fn uninstall() {
    let mut t = Test::new("uninstall", 156);
    let home = t.home.clone();

    // A home set up the way the installers and setup leave it, beside the user's own lines.
    write(&format!("{home}/.bashrc"), "alias ll='ls -l'\n");
    t.rant(&["setup", "bash"]).ok();
    append(&format!("{home}/.bashrc"), &format!("\nexport PATH=\"{home}/.rant/bin:$PATH\"    # rant-cli\n"));
    write(&format!("{home}/.config/fish/conf.d/rant.fish"), &format!("fish_add_path -g \"{home}/.rant/bin\"    # rant-cli\n"));
    write(&format!("{home}/.config/fish/conf.d/other.fish"), "set -g x 1\n");
    mkdir(&format!("{home}/.claude"));
    write(&t.path("tool/package.rant"), "name = \"tool\"\n");
    write(&t.path("tool/skill/SKILL.md"), "---\nname: tool\n---\n");
    t.rant(&["install", "tool", "--from", "tool"]).ok();
    write(&format!("{home}/.rant/packages/explorer/package.rant"), "description = \"The explorer\"\n");

    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.err, r"removing:\n");
    expect_match(&r.err, r"[.]bashrc [(]its rant lines[)]\n");
    expect_match(&r.err, r"conf[.]d.rant[.]fish\n");
    expect_match(&r.err, r"[.]claude.skills.tool [(]the skill of tool[)]\n");
    expect_match(&r.err, "Workspaces stay as they are");
    expect_match(&r.out, r"uninstalled rant-cli\n");

    // Only rant's own lines and files went.
    expect_match(&read(&format!("{home}/.bashrc")), r"^alias ll='ls -l'\n$");
    for gone in [format!("{home}/.rant"), format!("{home}/.claude/skills/tool"), format!("{home}/.config/fish/conf.d/rant.fish")] {
        assert!(!exists(&gone), "{gone} is still there");
    }
    for kept in [format!("{home}/.claude"), format!("{home}/.config/fish/conf.d/other.fish"), t.path("tool/skill/SKILL.md")] {
        assert!(exists(&kept), "{kept} was removed");
    }

    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.out, r"^rant-cli left nothing here to remove\n$");

    // The bridge config holds the user's passphrase, so it stays when the rest goes. No login
    // entry names this home, so the real one stays untouched.
    write(&format!("{home}/.rant/bridge.yaml"), "passphrase: secret\n");
    write(&format!("{home}/.rant/packages/bridge/package.rant"), "name = \"bridge\"\n");
    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.err, r"[.]rant\n");
    expect_match(&r.err, "bridge[.]yaml stays, it holds your bridge passphrase and settings");
    let left = list(&format!("{home}/.rant"));
    assert!(left.len() == 1 && exists(&format!("{home}/.rant/bridge.yaml")), "expected only bridge.yaml to stay, got: {left:?}");
    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.out, r"^rant-cli left nothing here to remove\n$");
    remove(&format!("{home}/.rant"));

    // A RANT_HOME not named .rant is never deleted whole.
    t.set_env("RANT_HOME", &home);
    let r = t.rant(&["uninstall"]).fails();
    expect_match(&r.err, "uninstall only removes a folder named [.]rant");
}
