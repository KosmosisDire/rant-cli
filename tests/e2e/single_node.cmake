include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(nodes)
install_test_node("demo/bin")

# A node starts detached and shows in ls.
rant(run node chatty)
expect_match("${OUT}" "^  [^ ]+ chatty +started +pid [0-9]+ +logs/chatty[.]log\n\n1 started, all running after 2 s\n$")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  chatty\n")
rant(sub chatter --count 2)
expect_match("${OUT}" "seq")

# Starting the same type again gives the copy a free name.
rant(run node chatty)
expect_match("${OUT}" "chatty_1 +started")
rant(stop chatty_1)
expect_match("${OUT}" "chatty_1 +stopped")

# It stops gracefully: the node itself saw the request and said so in its log.
rant(stop chatty)
expect_match("${OUT}" "chatty +stopped")
file(READ "${SCRATCH}/logs/chatty.log" log)
expect_match("${log}" "ready")
expect_match("${log}" "stopped")

# A node that ignores the request is killed after the grace period.
rant(run node stubborn)
rant(stop stubborn)
expect_match("${OUT}" "stubborn +killed +did not stop within 3 s")

# A node that exits within the watch fails the run with the end of its log, and is pruned
# at the next command.
rant(run node brief FAILS)
expect_match("${OUT}" "brief +exited after [0-9.]+ s +logs/brief[.]log\n")
expect_match("${OUT}" "brief, the end of logs/brief[.]log:\n")
expect_match("${OUT}" "0 of 1 running, brief exited[.] `rant log brief` shows why\n")
rant(ls)
expect_no_match("${OUT}" "brief")
rant(stop brief FAILS)
expect_match("${ERR}" "no node `brief` on domain")

# Mistakes are named.
rant(run node nope FAILS)
expect_match("${ERR}" "no node named `nope`")
rant(run chatty FAILS)
expect_match("${ERR}" "`chatty` is a node, run it in a group or alone with `rant run node chatty`")
rant(run FAILS)
expect_match("${ERR}" "say which group to run")

file(READ "${SCRATCH}/.rant/state" st)
expect_match("${st}" "\"instances\": \\[\\]")

# A node another workspace started stops from here too.
file(MAKE_DIRECTORY "${OTHER}")
file(COPY "${SCRATCH}/demo" DESTINATION "${OTHER}")
file(WRITE "${OTHER}/workspace.rant" "")
rant(run node chatty IN "${OTHER}")
rant(stop chatty)
expect_match("${OUT}" "chatty +stopped")
rant(ls IN "${OTHER}")
expect_no_match("${OUT}" "chatty")

# So does a node rant did not start, found on the mesh. Windows can only kill it.
rant_beside_node("--name;handmade" stop handmade)
expect_match("${OUT}" "handmade +(stopped|killed)")

# --all stops every node on the machine, whichever workspace started it, and so does a
# bare stop outside any workspace.
rant(run node chatty IN "${OTHER}")
rant(run node chatty)
rant(stop --all)
expect_match("${OUT}" "-other: +chatty\n")
expect_match("${ERR}" "Stop all 2 nodes")
rant(run node chatty IN "${OTHER}")
set(outside "${SCRATCH}-outside")
file(MAKE_DIRECTORY "${outside}")
rant(stop IN "${outside}")
expect_match("${ERR}" "Stop all 1 node[?]")
file(REMOVE_RECURSE "${outside}")
rant(stop --all)
expect_match("${OUT}" "no node is running on this machine")
