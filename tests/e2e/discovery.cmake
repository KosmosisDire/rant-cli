include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# A CMake package with a built node, a Python package and a declared node.
use_fixture(mixed)
install_test_node(cpp/build talker)

rant(build --dry-run)
expect_match("${OUT}" "PACKAGES\n  detector +py\n  lidar_driver +cpp\n  tools +tools\n\n")
expect_match("${OUT}" "NODE TYPES\n  detector/detector +python +py/detector.py\n  lidar_driver/talker +native +cpp/build/talker")
expect_match("${OUT}" "tools/echoer +declared +cmake -E echo hello\n")
expect_no_match("${OUT}" "helpers")

# The same from a subdirectory, as JSON with absolute paths.
rant(build --dry-run --json IN "${SCRATCH}/py")
expect_match("${OUT}" "\"name\": \"talker\"")
expect_match("${OUT}" "\"path\": \"[A-Za-z]*:?/[^\"]*/cpp/build/talker")

# The second run reads the cache.
if(NOT EXISTS "${SCRATCH}/.rant/cache/scan.json")
  message(FATAL_ERROR "no scan cache was written")
endif()

# A malformed rant.hcl is reported where the mistake is.
file(WRITE "${SCRATCH}/tools/rant.hcl" "package {\n  name = \n}\n")
rant(build --dry-run FAILS)
expect_match("${ERR}" "tools/rant.hcl:[23]:[0-9]+: syntax error")

# Outside any workspace the command says how to make one.
file(MAKE_DIRECTORY "${SCRATCH}/../outside-${DOMAIN}")
rant(build --dry-run FAILS IN "${SCRATCH}/../outside-${DOMAIN}")
expect_match("${ERR}" "rant init")
