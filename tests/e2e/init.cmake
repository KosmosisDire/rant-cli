include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

rant(init)
expect_match("${OUT}" "created .*rant.hcl")
file(READ "${SCRATCH}/rant.hcl" text)
expect_match("${text}" "workspace {")

rant(init FAILS)
expect_match("${ERR}" "exists already")
