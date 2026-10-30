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

# A node the service process starts, running already, has the environment of the rant run
# that asked for it.
file(WRITE "${SCRATCH}/tools/package.rant" "name = \"tools\"
node \"dump\" {
  run = [\"${CMAKE_COMMAND}\", \"-E\", \"environment\"]
  expect_mesh = false
}
")
file(WRITE "${SCRATCH}/Dump.run" "restart = \"on-failure\"
node \"tools/dump\" {}
")
set(ENV{RANT_TEST_MARK} "from-the-shell")
rant(run Dump --watch 0)
unset(ENV{RANT_TEST_MARK})
wait_for_service_log("dump exited with code 0")
file(READ "${SCRATCH}/logs/dump.log" dumped)
expect_match("${dumped}" "RANT_TEST_MARK=from-the-shell")
rant(stop Dump)

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
expect_match("${ERR}" "no services in workspace.rant, `rant enable <group>` adds one")
rant(enable nowhere FAILS)
expect_match("${ERR}" "no group named `nowhere`")
rant(enable sensor FAILS)
expect_match("${ERR}" "`sensor` is a node and a service is a group, `rant new group` makes one for it")
rant(enable node sensor FAILS)
expect_match("${ERR}" "a service is a group")
rant(enable Keep --dry-run)
expect_match("${OUT}" "keeper")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  [(]none[)] +not enabled +[.]\n")
file(READ "${SCRATCH}/workspace.rant" ws)
expect_no_match("${ws}" "service")

# A workspace lists its services, and a bare run starts each as an ordinary run of its group,
# as every boot will once the workspace is enabled. One that does not plan is named at its line.
file(WRITE "${SCRATCH}/Plain.run" [[
node "demo/sensor" {
  name = "steady"
}
]])
file(WRITE "${SCRATCH}/Desk.run" [[
param "seat" {}
node "demo/sensor" {
  name = "desk_${param.seat}"
}
]])
file(WRITE "${SCRATCH}/workspace.rant" "service \"Plain\" {}\nservice \"Desk\" {\n  seat = 1\n}\n")
rant(run FAILS)
expect_match("${ERR}" "workspace.rant:3:3: param `seat` is string, not an int")
expect_match("${OUT}" "steady +started")
file(WRITE "${SCRATCH}/workspace.rant" "service \"Plain\" {}\n\n# where the operator sits\nservice \"Desk\" {\n  seat = \"left\"\n}\n")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  Plain +not enabled +running[^\n]*\n  Desk seat=left +not enabled +stopped")
rant(run --dry-run)
expect_match("${OUT}" "desk_left")
rant(run)
expect_match("${OUT}" "steady +already running")
expect_match("${OUT}" "desk_left +started")

# Outside every workspace the services are those of services.rant in rant's home.
file(MAKE_DIRECTORY "${SCRATCH}-outside")
file(WRITE "$ENV{RANT_HOME}/services.rant" "service \"Nowhere\" {}\n")
rant(run IN "${SCRATCH}-outside" FAILS)
expect_match("${ERR}" "services.rant:1:1: no group named `Nowhere`")
file(REMOVE "$ENV{RANT_HOME}/services.rant")
rant(run IN "${SCRATCH}-outside" FAILS)
expect_match("${ERR}" "say which group to run")

# What rant enable keeps, written here by hand, since rant enable writes the real boot and
# login entries. An entry without a start starts at boot, and an older rant's entry for one
# group is passed over. The service process starts each workspace's services as the entries
# would.
rant(stop -y)
expect_match("${OUT}" "steady +stopped")
file(MAKE_DIRECTORY "${OTHER}")
file(COPY "${SCRATCH}/demo" DESTINATION "${OTHER}")
file(WRITE "${OTHER}/workspace.rant" "service \"Far\" {}\n")
file(WRITE "${OTHER}/Far.run" "node \"demo/sensor\" {\n  name = \"far\"\n}\n")
file(WRITE "$ENV{RANT_HOME}/services.json" "{ \"enabled\": [
  { \"folder\": \"${SCRATCH}\", \"start\": \"login\", \"domain\": ${DOMAIN} },
  { \"folder\": \"${OTHER}\", \"domain\": ${DOMAIN} },
  { \"folder\": \"${SCRATCH}\", \"kind\": \"group\", \"name\": \"Plain\", \"words\": [] }
] }\n")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  Plain +at login +stopped[^\n]*\n  Desk seat=left +at login +stopped[^\n]*\n  Far +at boot +stopped[^\n]*-other\n")

execute_process(COMMAND "${RANT}" __services --login "$ENV{RANT_HOME}" TIMEOUT 60)
wait_for_service_log("at login, starting the services of ")
expect_no_match("${LOG}" "starting the services of [^\n]*-other")
execute_process(COMMAND "${RANT}" __services --boot "$ENV{RANT_HOME}" TIMEOUT 60)
wait_for_service_log("at boot, starting the services of [^\n]*-other")
foreach(i RANGE 40)
  rant(ls nodes --no-mesh)
  set(here "${OUT}")
  rant(ls nodes --no-mesh IN "${OTHER}")
  if(here MATCHES "steady" AND here MATCHES "desk_left" AND OUT MATCHES "far")
    break()
  endif()
  execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 0.5)
endforeach()
expect_match("${here}" "Plain:\n +steady\n")
expect_match("${here}" "Desk:\n +desk_left\n")
expect_match("${OUT}" "Far:\n +far\n")
rant(ls services --json)
string(JSON state GET "${OUT}" 0 state)
expect_match("${state}" "^running$")
string(JSON start GET "${OUT}" 1 start)
expect_match("${start}" "^login$")
string(JSON params GET "${OUT}" 1 params 0)
expect_match("${params}" "^seat=left$")

# A stop or restart that names nothing leaves the services of an enabled workspace running.
file(WRITE "${SCRATCH}/Other.run" [[
node "demo/sensor" {
  name = "other"
}
]])
rant(run Other --watch 0)
rant(restart -y --no-build --watch 0)
expect_match("${OUT}" "roots: group Other\n")
expect_match("${ERR}" "services keep running, name one to restart it: Plain, Desk seat=left")
expect_match("${OUT}" "other +restarted")
expect_no_match("${OUT}" "steady|desk")
rant(stop -y)
expect_match("${OUT}" "nodes: +other\n")
expect_match("${ERR}" "services keep running, name one to stop it: Plain, Desk seat=left")
expect_match("${OUT}" "other +stopped")
rant(stop -y)
expect_match("${OUT}" "only services run here, name one to stop it: Plain, Desk seat=left")
rant(stop --all -y)
expect_match("${OUT}" "only services run on this machine, name one to stop it: Plain, Desk seat=left, Far")

# disable with a group removes its block and stops its run, keeping the rest of the file, and
# a group another workspace serves is pointed there.
rant(disable Desk)
expect_match("${OUT}" "desk_left +stopped")
expect_match("${ERR}" "removed Desk seat=left from the services in workspace.rant")
file(READ "${SCRATCH}/workspace.rant" ws)
expect_match("${ws}" "^service \"Plain\" {}\n\n# where the operator sits\n$")
rant(disable Desk FAILS)
expect_match("${ERR}" "`Desk` is not a service here")
rant(disable Far FAILS)
expect_match("${ERR}" "`Far` is a service of [^\n]*-other, disable it there")

# A bare disable stops the services and drops the start, and the blocks stay.
rant(disable)
expect_match("${OUT}" "steady +stopped")
expect_match("${ERR}" "disabled this workspace, its services no longer start when you log in")
file(READ "${SCRATCH}/workspace.rant" ws)
expect_match("${ws}" "service \"Plain\"")
rant(disable FAILS)
expect_match("${ERR}" "this workspace is not enabled")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  Far +at boot +running[^\n]*\n  Plain +not enabled +stopped")
rant(disable IN "${OTHER}")
expect_match("${OUT}" "far +stopped")
rant(ls services)
expect_match("${OUT}" "SERVICES\n  Plain +not enabled +stopped[^\n]*\n$")
wait_for_service_log("stopped, nothing to watch\n$")
