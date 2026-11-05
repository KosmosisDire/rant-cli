mod common;
use common::*;

const FAKE_CODE_CMD: &str = concat!(
    "@echo off\r\n",
    "echo %*>> \"%~dp0calls.txt\"\r\n",
    "if \"%~1\"==\"--install-extension\" echo test.ext@0.0.1> \"%~dp0listing.txt\"\r\n",
    "if \"%~1\"==\"--uninstall-extension\" del \"%~dp0listing.txt\"\r\n",
    "if \"%~1\"==\"--list-extensions\" if exist \"%~dp0listing.txt\" type \"%~dp0listing.txt\"\r\n",
    "exit /b 0\r\n",
);

const FAKE_CODE_SH: &str = concat!(
    "#!/bin/sh\n",
    "dir=$(dirname \"$0\")\n",
    "echo \"$*\" >> \"$dir/calls.txt\"\n",
    "case \"$1\" in\n",
    "  --install-extension) echo test.ext@0.0.1 > \"$dir/listing.txt\" ;;\n",
    "  --uninstall-extension) rm -f \"$dir/listing.txt\" ;;\n",
    "  --list-extensions) cat \"$dir/listing.txt\" 2>/dev/null ;;\n",
    "esac\n",
    "exit 0\n",
);

#[test]
fn vsix() {
    let mut t = Test::new("vsix", 157);
    let home = t.rant_home.clone();

    // A fake editor stands in for VS Code, so no test touches a real one. It logs its arguments
    // and keeps a listing the way `code --list-extensions --show-versions` prints one.
    let bin = t.path("editor");
    mkdir(&bin);
    let editor = if cfg!(windows) { format!("{bin}/fake-code.cmd") } else { format!("{bin}/fake-code") };
    write(&editor, if cfg!(windows) { FAKE_CODE_CMD } else { FAKE_CODE_SH });
    make_executable(&editor);
    let calls = format!("{bin}/calls.txt");

    // An extension's build folder: its package.rant and the .vsix its build wrote, a zip whose
    // extension/package.json names the extension.
    let build = t.path("ext-build");
    write(&format!("{build}/package.rant"), "name = \"ext\"\ndescription = \"A test extension\"\n");
    write(&t.path("zip/extension/package.json"), "{ \"name\": \"ext\", \"publisher\": \"Test\", \"version\": \"0.0.1\" }\n");
    tar(&t.path("zip"), "cf", &format!("{build}/ext-0.0.1.vsix"), &["extension"], Some("zip"));

    // The editor options belong to an extension, and a missing editor is refused before anything
    // is installed.
    write(&t.path("plain/package.rant"), "name = \"plain\"\ndescription = \"No extension\"\n");
    let r = t.rant(&["install", "plain", "--from", "plain", "--editor", &editor]).fails();
    expect_match(&r.err, "plain is no VS Code extension, so --editor and --profile do not apply");
    let r = t.rant(&["install", "ext", "--from", "ext-build", "--editor", "no-such-editor-here"]).fails();
    expect_match(&r.err, "no editor `no-such-editor-here` found");
    let left = list(&format!("{home}/packages"));
    assert!(left.is_empty(), "a refused install left {left:?}");

    // Installing goes through the editor and records where, beside the linked folder.
    let r = t.rant(&["install", "ext", "--from", "ext-build", "--editor", &editor]).ok();
    expect_match(&r.err, r"installed the extension test[.]ext 0[.]0[.]1 through [^\n]*fake-code");
    expect_match(&r.err, "linked ext to");
    expect_match(&read(&calls), r#"--install-extension [^\n]*ext-0[.]0[.]1[.]vsix"? --force"#);
    expect_match(&read(&format!("{home}/packages/ext.json")), r#""id": "test[.]ext""#);
    let r = t.rant(&["ls", "packages"]).ok();
    expect_match(&r.out, r"\n  ext +package[.]rant +linked");

    // Remove takes it out of the recorded editor, and the build folder stays.
    let r = t.rant(&["remove", "ext"]).ok();
    expect_match(&r.err, "removed ext");
    expect_match(&read(&calls), "--uninstall-extension test[.]ext");
    assert!(
        !exists(&format!("{home}/packages/ext")) && exists(&format!("{build}/ext-0.0.1.vsix")),
        "remove left the package or took the build"
    );

    // Profiles come from the editor's storage.json. The data folder of a portable install beside
    // the fake editor holds it, so no test reads a real one.
    t.unset_env("VSCODE_PORTABLE");
    write(&t.path("resources/app/product.json"), "{ \"nameShort\": \"Fake\" }\n");
    mkdir(&t.path("project"));
    let project_uri = if cfg!(windows) { format!("file:///{}/project", t.scratch) } else { format!("file://{}/project", t.scratch) };
    write(
        &t.path("data/user-data/User/globalStorage/storage.json"),
        &format!(
            r#"{{
  "userDataProfiles": [
    {{ "location": "builtin/agents", "name": "Agents", "useDefaultFlags": {{ "extensions": true }} }},
    {{ "location": "-500e13d6", "name": "Mixed" }}
  ],
  "profileAssociations": {{ "workspaces": {{ "{project_uri}": "-500e13d6" }} }}
}}
"#
        ),
    );
    remove(&calls);

    // Outside a VS Code terminal it asks among the profiles with their own extensions, and with
    // no one to answer it says how to pick.
    t.unset_env("TERM_PROGRAM");
    let r = t.rant(&["install", "ext", "--from", "ext-build", "--editor", &editor]).fails();
    expect_match(&r.err, "1  Default");
    expect_match(&r.err, "2  Mixed");
    expect_no_match(&r.err, "Agents");
    expect_match(&r.err, "pass --profile NAME");

    // A VS Code terminal takes the profile its folder opens in, and remove the recorded one.
    t.set_env("TERM_PROGRAM", "vscode");
    let r = t.rant(&["install", "ext", "--from", &build, "--editor", &editor]).cwd(&t.path("project")).ok();
    expect_match(&r.err, "in the profile Mixed");
    t.unset_env("TERM_PROGRAM");
    t.rant(&["remove", "ext"]).ok();
    let logged = read(&calls);
    expect_match(&logged, r"--install-extension [^\n]* --force --profile Mixed");
    expect_match(&logged, "--uninstall-extension test[.]ext --profile Mixed");

    // --profile picks one outright, and uninstall takes the extension out too.
    let r = t.rant(&["install", "ext", "--from", "ext-build", "--editor", &editor, "--profile", "Mixed"]).ok();
    expect_match(&r.err, "in the profile Mixed");
    let r = t.rant(&["uninstall"]).ok();
    expect_match(&r.err, r"the VS Code extension test[.]ext, through [^\n]*fake-code[^\n]* in the profile Mixed\n");
    let logged = read(&calls);
    assert_eq!(count_matches(&logged, "--uninstall-extension"), 2, "uninstall did not remove the extension:\n{logged}");
}
