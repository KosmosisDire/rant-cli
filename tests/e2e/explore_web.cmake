include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Nothing here may register a service or touch the registry: every case stops before the
# service is defined, and enabled means the definition names this scratch home's explorer.
set(home "${SCRATCH}/home/.rant")
set(ENV{HOME} "${SCRATCH}/home")
set(ENV{XDG_CONFIG_HOME} "${SCRATCH}/home/.config")
get_filename_component(ext "${RANT}" LAST_EXT)
if(NOT ext STREQUAL ".exe")
  set(ext "")
endif()

# An empty home has no explorer, and the server would listen on every address at 6732.
rant(explore web status)
expect_match("${OUT}" "installed: +false\n")
expect_match("${OUT}" "enabled: +false\n")
expect_match("${OUT}" "running: +false\n")
expect_match("${OUT}" "bind: +0[.]0[.]0[.]0\n")
expect_match("${OUT}" "port: +6732\n")
expect_match("${ERR}" "the explorer is not installed, run `rant install explorer`")

rant(explore web status --json)
foreach(key installed enabled running)
  string(JSON value GET "${OUT}" ${key})
  if(NOT value STREQUAL "OFF")
    message(FATAL_ERROR "${key} should be false in:\n${OUT}")
  endif()
endforeach()
foreach(key version url pid)
  string(JSON type TYPE "${OUT}" ${key})
  if(NOT type STREQUAL "NULL")
    message(FATAL_ERROR "${key} should be null in:\n${OUT}")
  endif()
endforeach()

# Stopping and disabling what is not there change nothing and say so.
rant(explore web stop)
expect_match("${ERR}" "the web explorer is not running")
rant(explore web disable)
expect_match("${ERR}" "the web explorer is not enabled")

# Without an explorer the others say what to do.
foreach(sub start restart run)
  rant(explore web ${sub} FAILS)
  expect_match("${ERR}" "the web explorer is not installed, run `rant install explorer`")
endforeach()

# A bad address is refused before anything is downloaded.
foreach(port 0 65536 x)
  rant(explore web enable --port "${port}" FAILS)
  expect_match("${ERR}" "--port takes a number from 1 to 65535")
endforeach()

# Enable never installs the explorer, rant install does.
rant(explore web enable FAILS)
expect_match("${ERR}" "the web explorer is not installed, run `rant install explorer`")

# An installed explorer from before the web server refuses --version, as the test node
# does, and is refused before the service is defined.
set(pkg "${home}/packages/explorer")
file(MAKE_DIRECTORY "${pkg}/bin")
file(COPY_FILE "${TEST_NODE}" "${pkg}/bin/rant_explorer${ext}")
file(WRITE "${pkg}/package.rant" "node \"explorer\" {\n  run = \"$\{path.package}/bin/rant_explorer\"\n}\n")
rant(explore web status --json)
string(JSON binary GET "${OUT}" binary)
expect_match("${binary}" "home/[.]rant/packages/explorer/bin/rant_explorer")
rant(explore web enable FAILS)
expect_match("${ERR}" "this explorer has no web server, `rant update explorer` installs the newest")
rant(explore web status --json)
string(JSON value GET "${OUT}" enabled)
if(NOT value STREQUAL "OFF")
  message(FATAL_ERROR "a refused explorer was enabled:\n${OUT}")
endif()

# A state file whose process is gone is a server that does not run.
file(WRITE "${home}/explorer-web.state" "pid: 2147483000\nurl: http://127.0.0.1:8090\nversion: 0.1.0\nstarted: 1760000000\n")
rant(explore web status --json)
string(JSON value GET "${OUT}" running)
if(NOT value STREQUAL "OFF")
  message(FATAL_ERROR "a stale state file should not count as running:\n${OUT}")
endif()
rant(explore web stop)
expect_match("${ERR}" "the web explorer is not running")

# The address enable saved is the one status shows.
file(WRITE "${home}/explorer-web.address" "127.0.0.1 9001\n")
rant(explore web status)
expect_match("${OUT}" "bind: +127[.]0[.]0[.]1\n")
expect_match("${OUT}" "port: +9001\n")

# The log of a server that logs to a file. Linux reads the journal instead.
if(NOT CMAKE_HOST_SYSTEM_NAME STREQUAL "Linux")
  rant(explore web logs FAILS)
  expect_match("${ERR}" "the web explorer has no log yet")
  file(MAKE_DIRECTORY "${home}/logs")
  file(WRITE "${home}/logs/explorer-web.log" "one\ntwo\nthree\n")
  rant(explore web logs)
  expect_match("${OUT}" "^one\ntwo\nthree\n$")
endif()

# Usage errors.
rant(explore web FAILS)
expect_match("${ERR}" "explore web needs a command: enable, disable, start, stop, restart, status, logs or run")
rant(explore web frobnicate FAILS)
expect_match("${ERR}" "unknown explore web command `frobnicate`")
rant(explore web status extra FAILS)
expect_match("${ERR}" "explore web status takes no arguments")
foreach(option port bind)
  rant(explore web start --${option} x FAILS)
  expect_match("${ERR}" "--${option} goes with `rant explore web enable`")
  rant(explore --${option} x FAILS)
  expect_match("${ERR}" "--${option} goes with `rant explore web enable`")
endforeach()
rant(explore web start -f FAILS)
expect_match("${ERR}" "--follow goes with `rant explore web logs`")
rant(explore -f FAILS)
expect_match("${ERR}" "--follow goes with `rant explore web logs`")
