include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(nodes)
install_test_node("demo/bin")

# A node starts detached and shows in ls.
rant(start node chatty)
expect_match("${OUT}" "^started chatty, pid [0-9]+, log logs/chatty.log")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  chatty\n")
rant(sub chatter --count 2)
expect_match("${OUT}" "seq")

# Starting the same type again gives the copy a free name.
rant(start node chatty)
expect_match("${OUT}" "^started chatty_1,")
rant(stop node chatty_1)
expect_match("${OUT}" "stopped chatty_1")

# It stops gracefully: the node itself saw the request and said so in its log.
rant(stop node chatty)
expect_match("${OUT}" "^stopped chatty\n")
file(READ "${SCRATCH}/logs/chatty.log" log)
expect_match("${log}" "ready")
expect_match("${log}" "stopped")

# A node that ignores the request is killed after the grace period.
rant(start node stubborn)
rant(stop node stubborn)
expect_match("${OUT}" "killed after")

# A node that exits on its own is pruned at the next command.
rant(start node brief)
execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 3)
rant(ls nodes)
expect_no_match("${OUT}" "brief")
rant(stop node brief FAILS)
expect_match("${ERR}" "no node named `brief` is running")

# Mistakes are named.
rant(start node nope FAILS)
expect_match("${ERR}" "no node named `nope`")
rant(start chatty FAILS)
expect_match("${ERR}" "rant start node")

file(READ "${SCRATCH}/.rant/state" st)
expect_match("${st}" "\"instances\": \\[\\]")
