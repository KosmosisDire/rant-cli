include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# An empty mesh lists nothing, and says so.
rant(ls)
expect_match("${OUT}" "NODES\n  [(]none[)]")

set(node "--name;talker;--pub;chatter;--var;speed;--fn;double;--task;count")

rant_beside_node("${node}" ls)
expect_match("${OUT}" "NODES\n  talker\n")
expect_match("${OUT}" "chatter +topic")
expect_match("${OUT}" "speed +var")
expect_match("${OUT}" "double +fn")
expect_match("${OUT}" "count +task")

rant_beside_node("${node}" ls entities cha --json)
expect_match("${OUT}" "\"name\": \"chatter\"")
expect_no_match("${OUT}" "speed|\"nodes\"")

# Piped, sub prints a YAML line per message, numbers in place from line to line.
rant_beside_node("${node}" sub chatter --count 3)
expect_match("${OUT}" "^{seq: +[0-9]+, value: +[0-9]+[.][0-9][0-9]}\n{seq: ")

rant_beside_node("${node}" sub chatter --count 2 --csv)
expect_match("${OUT}" "^seq,value\n[0-9]+,[0-9.]+\n[0-9]+,[0-9.]+\n$")

rant_beside_node("${node}" sub chatter --count 1 --json)
expect_match("${OUT}" "^{\"from\":\"talker\",\"value\":{\"seq\":[0-9]+,\"value\":[0-9.]+}}\n$")

rant_beside_node("${node}" info talker)
expect_match("${OUT}" "^node: +talker\n")
expect_match("${OUT}" "pid: +[0-9]+")
expect_match("${OUT}" "publishes: +[[]chatter[]]")

rant_beside_node("${node}" info speed)
expect_match("${OUT}" "^var: +speed\n")
expect_match("${OUT}" "type: +f64")
expect_match("${OUT}" "owner: +[[]talker[]]")
expect_match("${OUT}" "value: +1.50")

rant_beside_node("${node}" info chatter)
expect_match("${OUT}" "type: +[|]\n  Reading {\n    seq: +u32\n    value: f64\n  }\n")

rant(info nothing_here FAILS)
expect_match("${ERR}" "nothing named")
