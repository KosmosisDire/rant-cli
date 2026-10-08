include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(build)

# The dry run shows every edge with where it came from, the order and the exact commands.
rant(build --dry-run)
expect_match("${OUT}" "DEPENDENCIES\n  app -> driver +depend in app/rant.hcl\n  driver -> sdk +find_package[(]sdk[)] in driver/CMakeLists.txt\n")
expect_match("${OUT}" "BUILD ORDER\n  1. sdk\n +cmake -E echo building-sdk\n +cmake -E touch sdk.built\n  2. driver\n +cmake -S [.] -B build +[(]asks first[)]\n +cmake --build build\n  3. app\n")

# Building app builds what it needs first, in the order the dry run showed. The configure
# question is answered yes, since stdin is not a terminal.
rant(build app)
expect_match("${OUT}" "building sdk\n.*building-sdk\n.*building driver\n.*building-driver.*building app\n.*building-app\n.*built 3 packages")
expect_match("${ERR}" "driver has no build directory yet. Configure it")
if(NOT EXISTS "${SCRATCH}/sdk/sdk.built" OR NOT EXISTS "${SCRATCH}/driver/build/CMakeCache.txt")
  message(FATAL_ERROR "the builds did not run in their package directories")
endif()

# Once configured, a build only builds.
rant(build driver)
expect_no_match("${ERR}" "Configure")
expect_match("${OUT}" "built 2 packages")

# The first failure stops the run and names the package and the command.
file(WRITE "${SCRATCH}/broken/rant.hcl" "package {\n  build  = [\"cmake -E false\", \"cmake -E echo never\"]\n  depend = [\"sdk\"]\n}\n")
rant(build broken FAILS)
expect_match("${ERR}" "building broken failed: `cmake -E false` exited 1")
expect_no_match("${OUT}" "never")

# On Windows a batch file runs through cmd.exe, which must take every argument as data:
# no %VAR% expansion and no & to start a second command.
if(CMAKE_HOST_WIN32)
  file(WRITE "${SCRATCH}/batch/tool.cmd" "@echo off\n>args.txt echo(%1\n>>args.txt echo(%2\n")
  file(WRITE "${SCRATCH}/batch/rant.hcl"
       "package {\n  build = \"./tool.cmd %PATH% '&cmake -E touch pwned.txt'\"\n}\n")
  rant(build batch)
  file(READ "${SCRATCH}/batch/args.txt" args)
  expect_match("${args}" "^\"%PATH%\"\r?\n\"&cmake -E touch pwned.txt\"")
  if(EXISTS "${SCRATCH}/batch/pwned.txt")
    message(FATAL_ERROR "an argument ran as a command through cmd.exe")
  endif()
endif()

rant(build nope FAILS)
expect_match("${ERR}" "no package named `nope`")
