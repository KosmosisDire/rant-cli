include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

set(node "--name;writer;--pub;chatter;--var;speed;--fn;double;--task;count")

# Every entity kind is read and written from the shell, in the types the mesh declares.
rant_beside_node("${node}" get speed)
expect_match("${OUT}" "^1.50\n$")

rant_beside_node("${node}" get speed --csv)
expect_match("${OUT}" "^value\n1.5\n$")

rant_beside_node("${node}" set speed 3.25)
expect_match("${OUT}" "^3.25\n$")

rant_beside_node("${node}" call double 21)
expect_match("${OUT}" "^42.00\n$")

rant_beside_node("${node}" call count 3)
expect_match("${OUT}" "^ 1\n 2\n 3\n3\n$")
expect_match("${ERR}" "task [0-9]+ running, Ctrl-C cancels it")

rant_beside_node("${node}" call count 2 --json)
expect_match("${OUT}" "{\"progress\":1}\n{\"progress\":2}\n{\n  \"result\": 2\n}")

rant_beside_node("${node}" pub chatter seq=7 value=0.5)
expect_match("${ERR}" "published 1 message to chatter")

rant_beside_node("${node}" pub chatter "{seq: 8, value: 1}" --rate 20 --count 3)
expect_match("${ERR}" "published 3 messages to chatter")

# Mistakes name the entity, the field or the value.
rant(get speed FAILS)
expect_match("${ERR}" "no var named `speed` is on the mesh")
rant(set speed FAILS)
expect_match("${ERR}" "set takes a variable and a value")
rant(pub chatter "{seq: 1" FAILS)
expect_match("${ERR}" "cannot read the value")
