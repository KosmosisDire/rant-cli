include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

use_fixture(groups)
install_test_node("demo/bin")

# A group explains itself, and a dry run shows the resolved plan with absolute paths.
rant(run nav --help)
expect_match("${OUT}" "Usage: rant run nav")
expect_match("${OUT}" "Drive around")
expect_match("${OUT}" "speed=<float> +default 1")
expect_match("${OUT}" "range=<int> +default 30 +How far the lidar looks")
rant(run nav range=5 --dry-run --json)
expect_match("${OUT}" "\"name\": \"lidar\"")
expect_match("${OUT}" "\"range_5\"")
expect_match("${OUT}" "\"cwd\": \"[A-Za-z]*:?/[^\"]*/demo\"")

# Bad params never start anything.
rant(run nav speed=fast FAILS)
expect_match("${ERR}" "param `speed` takes a float")
rant(run nav colour=red FAILS)
expect_match("${ERR}" "group `nav` has no param `colour`")
rant(run nowhere FAILS)
expect_match("${ERR}" "no group named `nowhere`, see `rant ls groups`")

# Two groups share lidar and odom. A group named against the convention is noted.
rant(run nav)
expect_match("${OUT}" "^nav  range=30 speed=1\n\n.*lidar +started.*\n.*odom +started.*\n.*planner +started.*\n\n3 started, all running after 2 s\n$")
expect_match("${ERR}" "rename group `nav` to `NavGroup`")
rant(run nav)
expect_match("${OUT}" "lidar +already running\n.*odom +already running\n.*planner +already running\n\n3 already running\n")
rant(run pick)
expect_match("${OUT}" "lidar +already running +shared with nav\n.*odom +already running +shared with nav\n.*arm +started")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  nav:\n    lidar\n    odom\n    planner\n  pick:\n    lidar\n    odom\n    arm\n\nNOT RUNNING")
rant(ls entities)
expect_match("${OUT}" "scan +topic")
rant(info lidar)
expect_match("${OUT}" "roots: +[[]group nav range=30 speed=1, group pick[]]\n")

# Stopping one group leaves the shared nodes up.
rant(stop nav)
expect_match("${OUT}" "planner +stopped\n.*odom +kept running +still needed by pick\n.*lidar +kept running")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  pick:\n    lidar\n    odom\n    arm\n\nNOT RUNNING")
rant(sub scan --count 1)
expect_match("${OUT}" "seq")

# A node of the same name that rant did not start can stand in, after a question.
file(MAKE_DIRECTORY "${OTHER}")
file(COPY "${SCRATCH}/demo" DESTINATION "${OTHER}")
file(WRITE "${OTHER}/workspace.rant" "")
file(WRITE "${OTHER}/demo/package.rant"
     "name = \"demo\"\nnode \"planner\" {\n  run = [\"./bin/test_node\", \"--life\", \"60\"]\n}\n")
rant(run node planner IN "${OTHER}")
rant(run nav)
expect_match("${ERR}" "`planner` is already running on the mesh, but rant did not start it. Use it for nav\\? \\[y/N\\] y")
expect_match("${OUT}" "planner +already running +started outside rant")
rant(stop nav)
expect_no_match("${OUT}" "planner")
rant(ls nodes IN "${OTHER}")
expect_match("${OUT}" "NODES\n  planner\n")
rant(stop planner IN "${OTHER}")

# Stopping the last group stops everything, last started first.
rant(stop pick)
expect_match("${OUT}" "arm +stopped\n.*odom +stopped\n.*lidar +stopped\n")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  [(]none[)]")

# A bare stop lists everything rant started and stops it all after a question.
rant(run nav)
rant(run pick)
rant(stop)
expect_match("${OUT}" "groups: nav, pick\nnodes:  lidar, odom, planner, arm\n")
expect_match("${ERR}" "Stop all 4 nodes[?] [[]y/N[]] y")
expect_match("${OUT}" "arm +stopped\n.*planner +stopped\n.*odom +stopped\n.*lidar +stopped\n")
rant(stop)
expect_match("${OUT}" "nothing started here is running")

# Placement: prefixes join from the workspace in, and the node prefix is part of the name.
file(WRITE "${SCRATCH}/workspace.rant" "prefix = \"plant\"\n")
file(WRITE "${SCRATCH}/cell.run"
     "node_prefix = \"left\"\nprefix      = \"left\"\nnode \"sensor\" {\n  name = \"lidar\"\n}\n")
rant(run cell --dry-run --json)
expect_match("${OUT}" "\"name\": \"left/lidar\"")
expect_match("${OUT}" "\"RANT_PREFIX\": \"plant/left\"")
rant(run cell)
expect_match("${OUT}" "left/lidar +started")
rant(ls nodes)
expect_match("${OUT}" "cell:\n    left/lidar\n")
rant(stop cell)
expect_match("${OUT}" "left/lidar +stopped")
