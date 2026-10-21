include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Nodes with restart set run under the service process, which starts them again. Nothing
# here writes a login entry.
use_fixture(groups)
install_test_node("demo/bin")
file(WRITE "${SCRATCH}/Keep.run" [[
restart = "always"

node "demo/sensor" {
  name = "keeper"
  args = ["--life", "2", "--exit", "3"]
}

node "demo/sensor" {
  name    = "plain"
  restart = "never"
}
]])
file(WRITE "${SCRATCH}/Fail.run" [[
restart = "on-failure"

node "demo/sensor" {
  name = "good"
  args = ["--life", "2"]
}

node "demo/sensor" {
  name = "bad"
  args = ["--life", "2", "--exit", "1"]
}
]])

rant(run Keep --dry-run)
expect_match("${OUT}" "keeper .*\n  restart  always\n")
expect_no_match("${OUT}" "plain .*restart")

# The service process starts keeper, rant run plain, and both have a process at once.
rant(run Keep --watch 0)
expect_match("${OUT}" "keeper +started +pid [1-9]")
expect_match("${OUT}" "plain +started +pid [1-9]")
wait_for_service_log("started keeper in .*keeper exited with code 3, starting it again in 1 s.*started keeper again, restart 1\n")
expect_no_match("${LOG}" "plain")
file(READ "${SCRATCH}/.rant/state" state)
expect_match("${state}" "\"restarts\": [1-9]")

# A stopped node stays stopped, whatever its restart says.
rant(stop Keep)
file(READ "$ENV{RANT_HOME}/logs/services.log" before)
execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 4)
file(READ "$ENV{RANT_HOME}/logs/services.log" after)
string(LENGTH "${before}" length)
string(SUBSTRING "${after}" ${length} -1 since)
expect_no_match("${since}" "started keeper")
rant(ls nodes --no-mesh)
expect_no_match("${OUT}" "keeper")

# on-failure starts again only what failed.
rant(run Fail --watch 0)
wait_for_service_log("good exited with code 0, and its restart is on-failure")
wait_for_service_log("bad exited with code 1, starting it again")
rant(stop Fail)

# With nothing left to watch it ends on its own.
wait_for_service_log("stopped, nothing to watch\n$")
