include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(groups)
install_test_node("demo/bin")

rant(run nav)
string(REGEX MATCH "lidar +started +pid ([0-9]+)" _ "${OUT}")
set(first_pid "${CMAKE_MATCH_1}")

# A node comes back under its name with a new process.
rant(restart lidar)
string(REGEX MATCH "lidar +restarted +pid ([0-9]+)" _ "${OUT}")
if(NOT CMAKE_MATCH_1 OR CMAKE_MATCH_1 STREQUAL first_pid)
  message(FATAL_ERROR "lidar was not restarted as a new process:\n${OUT}")
endif()
rant(ls nodes)
expect_match("${OUT}" "NODES\n  nav:\n    planner\n    base:\n      lidar\n      odom\n\nNOT RUNNING")

# A group restarts as it is configured now: an edit is picked up, a dropped node stops.
file(READ "${SCRATCH}/nav.run" nav)
string(REPLACE "\"--fn\", \"speed_\${param.speed}\"" "\"--fn\", \"turn\"" nav "${nav}")
file(WRITE "${SCRATCH}/nav.run" "${nav}")
rant(restart nav)
expect_match("${OUT}" "lidar +restarted +pid [0-9]+.*\n.*odom +restarted +pid [0-9]+.*\n.*planner +restarted +pid [0-9]+")
rant(info node planner)
expect_match("${OUT}" "\"--fn\", turn[]]")

# Asked to restart everything, it asks first.
rant(restart)
expect_match("${OUT}" "roots: group nav range=30 speed=1\n")
expect_match("${ERR}" "Restart all 3 nodes[?] [[]y/N[]] y")

rant(restart nowhere FAILS)
expect_match("${ERR}" "`nowhere` is not running, starting it")
expect_match("${ERR}" "no node named `nowhere`")

# What is not running starts.
rant(restart nav speed=9)
expect_match("${ERR}" "group `nav` is not running, starting it")
expect_match("${OUT}" "planner")
rant(stop nav speed=9)
rant(restart sensor)
expect_match("${ERR}" "`sensor` is not running, starting it")
expect_match("${OUT}" "sensor +started +pid [0-9]+")

# A restart builds the node's package once it has stopped. A failed build starts the last
# build again and fails the restart.
file(READ "${SCRATCH}/demo/package.rant" demo)
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}build = \"cmake -E echo rebuilt-demo\"\n")
rant(restart sensor)
expect_match("${OUT}" "building demo\nrebuilt-demo\n")
expect_match("${OUT}" "sensor +restarted +pid [0-9]+")
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}build = \"cmake -E false\"\n")
rant(restart sensor FAILS)
expect_match("${ERR}" "building demo failed: .* exited 1, starting the last build")
expect_match("${OUT}" "sensor +restarted +pid [0-9]+")
rant(restart sensor --no-build)
expect_no_match("${OUT}" "building demo")
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}")

# A node that is not running, one that crashed say, is built before it starts too, as run
# builds what it starts.
rant(stop sensor)
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}build = \"cmake -E echo rebuilt-demo\"\n")
rant(restart sensor)
expect_match("${ERR}" "`sensor` is not running, starting it")
expect_match("${OUT}" "building demo\nrebuilt-demo\n")
expect_match("${OUT}" "sensor +started +pid [0-9]+")
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}")

# A node type means the node running under the name it starts with, never a copy beside it.
rant(restart demo/sensor)
expect_match("${OUT}" "sensor +restarted +pid [0-9]+")
rant(restart node demo/sensor)
expect_match("${OUT}" "sensor +restarted +pid [0-9]+")
rant(ls nodes)
expect_no_match("${OUT}" "sensor_1")
rant(restart demo/sensor range=1 FAILS)
expect_match("${ERR}" "a node takes no key=value")

# From another workspace it finds the node where it runs, as the node's name does.
file(MAKE_DIRECTORY "${OTHER}")
file(COPY "${SCRATCH}/demo" DESTINATION "${OTHER}")
file(WRITE "${OTHER}/workspace.rant" "")
rant(restart demo/sensor IN "${OTHER}")
expect_match("${ERR}" "`sensor` runs in")
expect_match("${OUT}" "sensor +restarted +pid [0-9]+")

# With none under that name it starts one, as with another type under that name.
rant(stop sensor)
rant(restart demo/sensor)
expect_match("${ERR}" "`demo/sensor` is not running, starting it")
expect_match("${OUT}" "sensor +started +pid [0-9]+")
rant(stop sensor)
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}node \"other\" {\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}\n")
file(WRITE "${SCRATCH}/Stand.run" "node \"demo/other\" {\n  name = \"sensor\"\n}\n")
rant(run Stand)
rant(restart demo/sensor)
expect_match("${OUT}" "sensor_1 +started +pid [0-9]+")
rant(stop Stand)
rant(stop sensor_1)
file(REMOVE "${SCRATCH}/Stand.run")
file(WRITE "${SCRATCH}/demo/package.rant" "${demo}")

# Roots sharing a node start it once. A second start leaves the first running untracked, which
# stays on the mesh after a stop.
rant(run pick)
rant(restart)
expect_match("${OUT}" "\n4 restarted, all running")
rant(run nav speed=9)
rant(restart nav)
expect_match("${OUT}" "\n3 restarted, all running")

rant(stop)
rant(ls nodes)
expect_match("${OUT}" "NODES\n  [(]none[)]")
