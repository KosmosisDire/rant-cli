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
file(READ "${SCRATCH}/nav.hcl" nav)
string(REPLACE "\"--fn\", \"speed_\${param.speed}\"" "\"--fn\", \"turn\"" nav "${nav}")
file(WRITE "${SCRATCH}/nav.hcl" "${nav}")
rant(restart group nav)
expect_match("${OUT}" "restarted lidar, pid [0-9]+\nrestarted odom, pid [0-9]+\nrestarted planner, pid [0-9]+\n")
rant(info node planner)
expect_match("${OUT}" "\"--fn\", turn[]]")

# Asked to restart everything, it asks first.
rant(restart)
expect_match("${OUT}" "roots: group nav range=30 speed=1\n")
expect_match("${ERR}" "Restart all 3 nodes[?] [[]y/N[]] y")

rant(restart node nowhere FAILS)
expect_match("${ERR}" "rant runs no node named `nowhere`")
rant(restart group nav speed=9 FAILS)
expect_match("${ERR}" "`nav range=30 speed=9` is not running")
rant(restart group FAILS)
expect_match("${ERR}" "say what to restart")

rant(stop)
