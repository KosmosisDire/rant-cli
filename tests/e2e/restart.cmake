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
expect_match("${OUT}" "NODES\n  nav:\n    lidar\n    odom\n    planner\n\nNOT RUNNING")

# A group restarts as it is configured now: an edit is picked up, a dropped node stops.
file(READ "${SCRATCH}/nav.rungroup" nav)
string(REPLACE "\"--fn\", \"speed_\${param.speed}\"" "\"--fn\", \"turn\"" nav "${nav}")
file(WRITE "${SCRATCH}/nav.rungroup" "${nav}")
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

rant(stop)
