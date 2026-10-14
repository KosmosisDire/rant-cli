include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(groups)
install_test_node("demo/bin")

rant(run nav)

# One node's log is printed as it is. The test node prints its own default name.
rant(log lidar)
expect_match("${OUT}" "^ready test_node\n$")
rant(log lidar -n 0)
expect_match("${OUT}" "^$")

# A group's logs, or every running node's, interleave with each line led by its node.
rant(log nav)
expect_match("${OUT}" "^lidar   [|] ready test_node\nodom    [|] ready test_node\nplanner [|] ready test_node\n$")
rant(log)
expect_match("${OUT}" "lidar   [|] ready test_node\n")

# The log of a node that stopped stays readable.
rant(stop nav)
rant(log lidar)
expect_match("${OUT}" "^ready test_node\nstopped\n$")

rant(log nowhere FAILS)
expect_match("${ERR}" "`nowhere` has no log")
