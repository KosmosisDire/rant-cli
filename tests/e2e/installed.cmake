include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# An installed package is a folder in rant's home holding its package.rant, its program and
# the groups beside them, as rant install leaves it.
set(PKG "$ENV{RANT_HOME}/packages/tool")
install_test_node("home/.rant/packages/tool/bin")
file(WRITE "${PKG}/package.rant" "description = \"Answers questions\"\narchive = \"tool-$\{version}.zip\"\nnode \"tool\" {\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}\n")
file(WRITE "${PKG}/Tool.run" "node \"tool\" {}\n")

# Outside every workspace the installed packages stand alone, run from rant's home.
rant(ls packages)
expect_match("${OUT}" "^PACKAGES\n  tool +package[.]rant +installed +")
rant(ls groups)
expect_match("${OUT}" "^GROUPS\n  tool/Tool")
rant(info package tool)
expect_match("${OUT}" "\ndescription: Answers questions\n")
rant(run Tool)
expect_match("${OUT}" "tool +started")
rant(log tool)
expect_match("${OUT}" "^ready test_node\n$")
if(NOT EXISTS "$ENV{RANT_HOME}/logs/tool.log")
  message(FATAL_ERROR "the log of a node run outside a workspace belongs in rant's home")
endif()
rant(stop Tool)
expect_match("${OUT}" "tool +stopped")
rant(build FAILS)
expect_match("${ERR}" "no workspace here")

# A workspace sees them after its own packages, and never builds one.
use_fixture(groups)
install_test_node("demo/bin")
rant(ls packages)
expect_match("${OUT}" "\n  demo +package[.]rant +no Rant +demo\n  tool +package[.]rant +installed +")
rant(build --dry-run)
expect_no_match("${OUT}" "tool")
rant(build tool FAILS)
expect_match("${ERR}" "`tool` is installed, it came built")
rant(run node tool)
expect_match("${OUT}" "tool +started")
rant(stop tool)

# A package of the workspace hides the installed one of the same name.
file(WRITE "${SCRATCH}/tool/package.rant" "node \"mine\" {\n  run = \"mine\"\n}\n")
rant(ls packages tool --json)
expect_match("${OUT}" "\"installed\": false")
expect_no_match("${OUT}" "\"installed\": true")
