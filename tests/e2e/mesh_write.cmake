include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

set(node "--name;writer;--pub;chatter;--var;speed;--fn;double;--task;count")

# Every entity kind is read and written from the shell, in the types the mesh declares. sub
# and pub take a variable as a topic: its value at once from its owner, and a write the owner applied.
rant_beside_node("${node}" sub speed --count 1)
expect_match("${OUT}" "^{from: writer, hz: +0[.]00, value: +1[.]50}\n$")

rant_beside_node("${node}" sub speed --count 1 --json)
expect_match("${OUT}" "^{\"from\":\"writer\",\"hz\":0[.]0,\"value\":1[.]5}\n$")

rant_beside_node("${node}" sub speed --count 1 --csv)
expect_match("${OUT}" "^from,hz,value\nwriter,0[.]0,1[.]5\n$")

rant_beside_node("${node}" pub speed 3.25)
expect_match("${ERR}" "wrote 1 value to speed")

rant_beside_node("${node}" call double 21)
expect_match("${OUT}" "^42.00\n$")

# A task's progress is a stream as sub prints it, and the result is the last line.
rant_beside_node("${node}" call count 3)
expect_match("${OUT}" "^{from: writer, hz: +[0-9.]+, value: +1}\n{from: writer, hz: +[0-9.]+, value: +2}\n{from: writer, hz: +[0-9.]+, value: +3}\n3\n$")
expect_match("${ERR}" "task [0-9]+ running, Ctrl-C cancels it")

rant_beside_node("${node}" call count 2 --json)
expect_match("${OUT}" "^{\"from\":\"writer\",\"hz\":[0-9.]+,\"value\":1}\n{\"from\":\"writer\",\"hz\":[0-9.]+,\"value\":2}\n2\n$")

rant_beside_node("${node}" call count 2 --csv)
expect_match("${OUT}" "^value\n2\n$")

rant_beside_node("${node}" call count 2 --value-only --json)
expect_match("${OUT}" "^1\n2\n2\n$")

rant_beside_node("${node}" pub chatter seq=7 value=0.5)
expect_match("${ERR}" "published 1 message to chatter")

rant_beside_node("${node}" pub chatter "{seq: 8, value: 1}" --rate 20 --count 3)
expect_match("${ERR}" "published 3 messages to chatter")

# Mistakes name the entity, the field or the value.
rant(pub speed 1 FAILS)
expect_match("${ERR}" "no topic or var named `speed` is on the mesh")
rant(pub speed FAILS)
expect_match("${ERR}" "pub takes a topic or variable and a value")
rant(pub chatter "{seq: 1" FAILS)
expect_match("${ERR}" "cannot read the value")
