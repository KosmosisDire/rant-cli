// PLCnext Engineer runs on Windows only, and elsewhere a library is never placed.
#![cfg(windows)]

mod common;
use common::*;

#[test]
fn pcwlx() {
    let mut t = Test::new("pcwlx", 164);
    let home = t.rant_home.clone();

    // Engineer's settings in a scratch appdata name the folder it takes user libraries from, so no
    // test touches the real one.
    let libs = t.path("engineer-libs");
    t.set_env("APPDATA", &t.path("appdata"));
    let config = format!("<ShellConfiguration>\n  <LibraryPath>{}</LibraryPath>\n</ShellConfiguration>\n", libs.replace('/', "\\"));
    write(&t.path("appdata/PHOENIX CONTACT/PLCnext Engineer/2026.3/User.config"), &config);

    // A library's build folder: its package.rant and the .pcwlx its build wrote in bin, a zip
    // whose Core.xml names the library.
    let build = t.path("lib-build");
    write(&format!("{build}/package.rant"), "name = \"plclib\"\ndescription = \"A test library\"\n");
    write(&t.path("zip/_properties/Core.xml"), "<coreProperties><dc:title>Testlib</dc:title></coreProperties>\n");
    mkdir(&format!("{build}/bin"));
    tar(&t.path("zip"), "cf", &format!("{build}/bin/Testlib.pcwlx"), &["_properties"], Some("zip"));

    // Installing places it in its own folder there and records it, beside the linked folder.
    let placed = format!("{libs}/Testlib/Testlib.pcwlx");
    let r = t.rant(&["install", "plclib", "--from", "lib-build"]).ok();
    expect_match(&r.err, r"installed the PLCnext Engineer library Testlib as [^\n]*engineer-libs/Testlib/Testlib[.]pcwlx");
    expect_match(&r.err, "linked plclib to");
    assert!(exists(&placed), "install placed no library");
    expect_match(&read(&format!("{home}/packages/plclib.json")), r#""engineer_libraries": \["#);

    // Installing a new build again replaces the copy at the same path.
    write(&t.path("zip/_properties/Additional.xml"), "<AdditionalProperties />\n");
    tar(&t.path("zip"), "cf", &format!("{build}/bin/Testlib.pcwlx"), &["_properties"], Some("zip"));
    let r = t.rant(&["install", "plclib", "--from", "lib-build"]).ok();
    expect_match(&r.err, "installed the PLCnext Engineer library Testlib");
    let bytes = |p: &str| std::fs::read(p).unwrap();
    assert_eq!(bytes(&placed), bytes(&format!("{build}/bin/Testlib.pcwlx")), "install kept the old library");
    assert_eq!(list(&format!("{libs}/Testlib")).len(), 1, "install left more than the library");

    // Remove takes it out of Engineer's folder, and the build stays.
    let r = t.rant(&["remove", "plclib"]).ok();
    expect_match(&r.err, "removed plclib");
    assert!(!exists(&placed) && !exists(&format!("{libs}/Testlib")), "remove left the library");
    assert!(exists(&format!("{build}/bin/Testlib.pcwlx")), "remove took the build");
}
