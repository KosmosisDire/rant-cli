include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(groups)
install_test_node("demo/bin")

rant(start group nav)
string(REGEX MATCH "started lidar, pid ([0-9]+)" _ "${OUT}")
set(first_pid "${CMAKE_MATCH_1}")

# A node comes back under its name with a new process.
rant(restart node lidar)
string(REGEX MATCH "^restarted lidar, pid ([0-9]+)\n$" _ "${OUT}")
if(NOT CMAKE_MATCH_1 OR CMAKE_MATCH_1 STREQUAL first_pid)
  message(FATAL_ERROR "lidar was not restarted as a new process:\n${OUT}")
endif()
rant(ls nodes)
expect_match("${OUT}" "NODES\n  nav:\n    lidar\n    odom\n    planner\n$")

# A group restarts as it is configured now: an edit is picked up, a dropped node stops.
file(READ "${SCRATCH}/nav.group.hcl" nav)
string(REPLACE "\"--fn\", \"speed_\${param.speed}\"" "\"--fn\", \"turn\"" nav "${nav}")
file(WRITE "${SCRATCH}/nav.group.hcl" "${nav}")
rant(restart group nav)
expect_match("${OUT}" "restarted lidar, pid [0-9]+\nrestarted odom, pid [0-9]+\nrestarted planner, pid [0-9]+\n")
rant(info node planner)
expect_match("${OUT}" "\"--fn\", turn[]]")

# Asked to restart everything, it asks first.
rant(restart)
expect_match("${OUT}" "roots: group nav range=30 speed=1\n")
expect_match("${ERR}" "Restart all 3 nodes[?] [[]y/N[]] y")

rant(restart node nowhere FAILS)
expect_match("${ERR}" "`nowhere` is not running, starting it")
expect_match("${ERR}" "no node named `nowhere`")

# What is not running starts.
rant(restart group nav speed=9)
expect_match("${ERR}" "group `nav` is not running, starting it")
expect_match("${OUT}" "planner")
rant(stop group nav speed=9)
rant(restart node sensor)
expect_match("${ERR}" "`sensor` is not running, starting it")
expect_match("${OUT}" "started sensor, pid [0-9]+")
rant(restart group FAILS)
expect_match("${ERR}" "say what to restart")

rant(stop)
