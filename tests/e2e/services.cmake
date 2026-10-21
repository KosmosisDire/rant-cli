include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Nodes with restart set run under the service process, which starts them again. Nothing
# here writes a boot or login entry.
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

# rant enable refuses what it cannot run before it keeps anything or writes an entry, and a
# dry run keeps nothing.
rant(enable FAILS)
expect_match("${ERR}" "say what to enable")
rant(enable nowhere FAILS)
expect_match("${ERR}" "no group named `nowhere`")
rant(enable sensor FAILS)
expect_match("${ERR}" "`sensor` is a node, enable it with `rant enable node sensor`")
rant(enable Keep --dry-run)
expect_match("${OUT}" "keeper")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  [(]none[)]\n")

# What rant enable keeps, written here by hand, since rant enable writes the real boot and
# login entries. An entry without a start starts at boot. The service process starts each
# as the entries would.
file(WRITE "${SCRATCH}/Plain.run" [[
node "demo/sensor" {
  name = "steady"
}
]])
file(WRITE "${SCRATCH}/Desk.run" [[
node "demo/sensor" {
  name = "desk"
}
]])
file(WRITE "$ENV{RANT_HOME}/services.json" "{ \"enabled\": [
  { \"folder\": \"${SCRATCH}\", \"kind\": \"group\", \"name\": \"Plain\", \"words\": [], \"domain\": ${DOMAIN} },
  { \"folder\": \"${SCRATCH}\", \"start\": \"login\", \"kind\": \"group\", \"name\": \"Desk\", \"words\": [], \"domain\": ${DOMAIN} }
] }\n")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  Plain +at boot +stopped[^\n]*\n  Desk +at login +stopped")

execute_process(COMMAND "${RANT}" __services --login "$ENV{RANT_HOME}" TIMEOUT 60)
wait_for_service_log("starting Desk in .*started at login\n")
expect_no_match("${LOG}" "starting Plain")
execute_process(COMMAND "${RANT}" __services --boot "$ENV{RANT_HOME}" TIMEOUT 60)
wait_for_service_log("starting Plain in .*started at boot\n")
foreach(i RANGE 40)
  rant(ls nodes --no-mesh)
  if(OUT MATCHES "steady" AND OUT MATCHES "desk")
    break()
  endif()
  execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 0.5)
endforeach()
expect_match("${OUT}" "Plain:\n +steady\n")
expect_match("${OUT}" "Desk:\n +desk\n")
rant(ls services --json)
string(JSON state GET "${OUT}" 0 state)
expect_match("${state}" "^running$")
string(JSON start GET "${OUT}" 1 start)
expect_match("${start}" "^login$")

# disable stops it and drops it from the boot or login start.
rant(disable Plain)
expect_match("${OUT}" "steady +stopped")
expect_match("${ERR}" "disabled Plain, it no longer starts when this computer starts")
rant(disable Desk)
expect_match("${ERR}" "disabled Desk, it no longer starts when you log in")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  [(]none[)]\n")
rant(disable Plain FAILS)
expect_match("${ERR}" "Plain is not enabled here")
wait_for_service_log("stopped, nothing to watch\n$")
