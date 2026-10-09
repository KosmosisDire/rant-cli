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
expect_match("${ERR}" "no node `brief` on domain")

# Mistakes are named.
rant(start node nope FAILS)
expect_match("${ERR}" "no node named `nope`")
rant(start chatty FAILS)
expect_match("${ERR}" "rant start node")

file(READ "${SCRATCH}/.rant/state" st)
expect_match("${st}" "\"instances\": \\[\\]")

# A node another workspace started stops from here too.
file(MAKE_DIRECTORY "${OTHER}")
file(COPY "${SCRATCH}/demo" DESTINATION "${OTHER}")
file(WRITE "${OTHER}/rant.hcl" "workspace {}\n")
rant(start node chatty IN "${OTHER}")
rant(stop node chatty)
expect_match("${OUT}" "^stopped chatty\n")
rant(ls nodes IN "${OTHER}")
expect_no_match("${OUT}" "chatty")

# So does a node rant did not start, found on the mesh. Windows can only kill it.
rant_beside_node("--name;handmade" stop node handmade)
expect_match("${OUT}" "(stopped|killed) handmade")

# --all stops every node on the machine, whichever workspace started it, and so does a
# bare stop outside any workspace.
rant(start node chatty IN "${OTHER}")
rant(start node chatty)
rant(stop --all)
expect_match("${OUT}" "-other: +chatty\n")
expect_match("${ERR}" "Stop all 2 nodes")
rant(start node chatty IN "${OTHER}")
set(outside "${SCRATCH}-outside")
file(MAKE_DIRECTORY "${outside}")
rant(stop IN "${outside}")
expect_match("${ERR}" "Stop all 1 node[?]")
file(REMOVE_RECURSE "${outside}")
rant(stop --all)
expect_match("${OUT}" "no node is running on this machine")
