include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

rant(init)
expect_match("${OUT}" "created .*workspace.rant")
file(READ "${SCRATCH}/workspace.rant" text)
expect_match("${text}" "^# logs")

rant(init FAILS)
expect_match("${ERR}" "exists already")
