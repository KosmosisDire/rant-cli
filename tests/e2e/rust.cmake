include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

find_program(CARGO cargo)
if(NOT CARGO)
  message("SKIPPED: cargo is not installed")
  return()
endif()

use_fixture(rust)

# Each program of the Cargo package is a node before any build, and builds with cargo build.
rant(build --dry-run)
expect_match("${OUT}" "motor/jog +rust +motor/src/bin/jog[.]rs")
expect_match("${OUT}" "motor/motor +rust +motor/src/main[.]rs")
expect_match("${OUT}" "1. motor\n +cargo build\n")

# It cannot start before it is built.
rant(run node motor FAILS)
expect_match("${ERR}" "`motor/motor` is not built yet, run `rant build motor` first")

# Built, it runs from the target folder, and the copies cargo keeps in it are no nodes. This
# stand in never joins the mesh, so the run does not wait for it.
rant(build)
expect_match("${OUT}" "built 1 package")
rant(build --dry-run)
expect_match("${OUT}" "motor/motor +rust +motor/target/debug/motor")
rant(ls nodes --no-mesh --json)
expect_no_match("${OUT}" "motor-[0-9a-f]")
rant(run node motor --wait 0)
expect_match("${OUT}" "motor +started")
rant(stop motor)
expect_match("${OUT}" "motor +stopped\n")
file(READ "${SCRATCH}/logs/motor.log" log)
expect_match("${log}" "ready motor")

# rant lib lists the use, and leaves a path one to its checkout.
rant(lib)
expect_match("${OUT}" "motor +rust +none +Cargo path")
