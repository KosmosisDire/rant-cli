include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Nothing here may register a service or touch the registry: every case stops before the
# service is defined, and enabled means the definition names this scratch home's explorer.
set(home "${SCRATCH}/home/.rant")
set(ENV{HOME} "${SCRATCH}/home")
set(ENV{XDG_CONFIG_HOME} "${SCRATCH}/home/.config")

# An empty home has no explorer, and the server would listen on every address at 8090.
rant(web status)
expect_match("${OUT}" "installed: +false\n")
expect_match("${OUT}" "enabled: +false\n")
expect_match("${OUT}" "running: +false\n")
expect_match("${OUT}" "bind: +0[.]0[.]0[.]0\n")
expect_match("${OUT}" "port: +8090\n")
expect_match("${ERR}" "the explorer is not installed, run `rant web enable`")

rant(web status --json)
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
string(JSON binary GET "${OUT}" binary)
expect_match("${binary}" "home/[.]rant/bin/rant-explorer")

# Stopping and disabling what is not there change nothing and say so.
rant(web stop)
expect_match("${ERR}" "the web explorer is not running")
rant(web disable)
expect_match("${ERR}" "the web explorer is not enabled")

# Without an explorer the others say what to do.
foreach(sub start restart run)
  rant(web ${sub} FAILS)
  expect_match("${ERR}" "the web explorer is not installed, run `rant web enable`")
endforeach()

# A bad address is refused before anything is downloaded.
foreach(port 0 65536 x)
  rant(web enable --port "${port}" FAILS)
  expect_match("${ERR}" "--port takes a number from 1 to 65535")
endforeach()
if(EXISTS "${home}/bin")
  message(FATAL_ERROR "a refused address installed an explorer")
endif()

# No release to download: the proxy that refuses every connection stands in for no network.
set(ENV{HTTPS_PROXY} "http://127.0.0.1:9")
set(ENV{https_proxy} "http://127.0.0.1:9")
rant(web enable FAILS)
expect_match("${ERR}" "rant web enable --from PATH")
unset(ENV{HTTPS_PROXY})
unset(ENV{https_proxy})

# A file that is not an explorer is refused before anything is installed.
rant(web enable --from missing FAILS)
expect_match("${ERR}" "no file `missing`")
file(WRITE "${SCRATCH}/not-an-explorer.txt" "text\n")
rant(web enable --from not-an-explorer.txt FAILS)
expect_match("${ERR}" "does not run here")
if(EXISTS "${home}/bin")
  message(FATAL_ERROR "a refused file was installed")
endif()

# A program that runs but serves no page, as an explorer from before the web server, is
# refused before the service is defined. The CLI itself stands in for one.
rant(web enable --from "${RANT}" FAILS)
expect_match("${ERR}" "has no web server, `rant update` installs the newest explorer")
rant(web status --json)
string(JSON value GET "${OUT}" enabled)
if(NOT value STREQUAL "OFF")
  message(FATAL_ERROR "a refused explorer was enabled:\n${OUT}")
endif()

# A state file whose process is gone is a server that does not run.
file(MAKE_DIRECTORY "${home}")
file(WRITE "${home}/explorer-web.state" "pid: 2147483000\nurl: http://127.0.0.1:8090\nversion: 0.1.0\nstarted: 1760000000\n")
rant(web status --json)
string(JSON value GET "${OUT}" running)
if(NOT value STREQUAL "OFF")
  message(FATAL_ERROR "a stale state file should not count as running:\n${OUT}")
endif()
rant(web stop)
expect_match("${ERR}" "the web explorer is not running")

# The address enable saved is the one status shows.
file(WRITE "${home}/explorer-web.address" "127.0.0.1 9001\n")
rant(web status)
expect_match("${OUT}" "bind: +127[.]0[.]0[.]1\n")
expect_match("${OUT}" "port: +9001\n")

# The log of a server that logs to a file. Linux reads the journal instead.
if(NOT CMAKE_HOST_SYSTEM_NAME STREQUAL "Linux")
  rant(web logs FAILS)
  expect_match("${ERR}" "the web explorer has no log yet")
  file(MAKE_DIRECTORY "${home}/logs")
  file(WRITE "${home}/logs/explorer-web.log" "one\ntwo\nthree\n")
  rant(web logs)
  expect_match("${OUT}" "^one\ntwo\nthree\n$")
endif()

# Usage errors.
rant(web FAILS)
expect_match("${ERR}" "web needs a command")
rant(web frobnicate FAILS)
expect_match("${ERR}" "unknown web command `frobnicate`")
rant(web status extra FAILS)
expect_match("${ERR}" "web status takes no arguments")
foreach(option from port bind)
  rant(web start --${option} x FAILS)
  expect_match("${ERR}" "--${option} goes with `rant web enable`")
endforeach()
rant(web start -f FAILS)
expect_match("${ERR}" "--follow goes with `rant web logs`")
