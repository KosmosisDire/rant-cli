include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Nothing here may register a service or touch the registry: every case stops before the
# service is defined, and enabled means the definition names this scratch home's bridge.
set(home "${SCRATCH}/home/.rant")
set(ENV{HOME} "${SCRATCH}/home")
set(ENV{XDG_CONFIG_HOME} "${SCRATCH}/home/.config")

# An empty home has no bridge.
rant(bridge status)
expect_match("${OUT}" "installed: +false\n")
expect_match("${OUT}" "enabled: +false\n")
expect_match("${OUT}" "running: +false\n")
expect_match("${OUT}" "bridge[.]yaml\n")
expect_match("${ERR}" "the bridge is not installed, run `rant bridge enable`")

rant(bridge status --json)
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
string(JSON config GET "${OUT}" config)
expect_match("${config}" "home/[.]rant/bridge[.]yaml$")
string(JSON binary GET "${OUT}" binary)
expect_match("${binary}" "home/[.]rant/bin/rant-bridge")

# Stopping and disabling what is not there change nothing and say so.
rant(bridge stop)
expect_match("${ERR}" "the bridge is not running")
rant(bridge disable)
expect_match("${ERR}" "the bridge is not enabled")

# Without a bridge the others say what to do.
foreach(sub start restart run)
  rant(bridge ${sub} FAILS)
  expect_match("${ERR}" "the bridge is not installed, run `rant bridge enable`")
endforeach()

# No release to download: the proxy that refuses every connection stands in for no network
# or no release, so this holds once a bridge release exists.
set(ENV{HTTPS_PROXY} "http://127.0.0.1:9")
set(ENV{https_proxy} "http://127.0.0.1:9")
rant(bridge enable FAILS)
expect_match("${ERR}" "rant bridge enable --from PATH")
unset(ENV{HTTPS_PROXY})
unset(ENV{https_proxy})

# A file that is not a bridge is refused before anything is installed.
rant(bridge enable --from missing FAILS)
expect_match("${ERR}" "no file `missing`")
file(WRITE "${SCRATCH}/not-a-bridge.txt" "text\n")
rant(bridge enable --from not-a-bridge.txt FAILS)
expect_match("${ERR}" "does not run here")
if(EXISTS "${home}/bin")
  message(FATAL_ERROR "a refused file was installed")
endif()

# A state file whose process is gone is a bridge that does not run.
file(MAKE_DIRECTORY "${home}")
file(WRITE "${home}/bridge.state" "pid: 2147483000\nurl: ws://127.0.0.1:7480\nversion: 0.1.0\nstarted: 1760000000\n")
rant(bridge status --json)
string(JSON value GET "${OUT}" running)
if(NOT value STREQUAL "OFF")
  message(FATAL_ERROR "a stale state file should not count as running:\n${OUT}")
endif()
rant(bridge stop)
expect_match("${ERR}" "the bridge is not running")

# The log of a bridge that logs to a file. Linux reads the journal instead.
if(NOT CMAKE_HOST_SYSTEM_NAME STREQUAL "Linux")
  rant(bridge logs FAILS)
  expect_match("${ERR}" "the bridge has no log yet")
  file(MAKE_DIRECTORY "${home}/logs")
  file(WRITE "${home}/logs/bridge.log" "one\ntwo\nthree\n")
  rant(bridge logs)
  expect_match("${OUT}" "^one\ntwo\nthree\n$")
endif()

# Usage errors.
rant(bridge FAILS)
expect_match("${ERR}" "bridge needs a command")
rant(bridge frobnicate FAILS)
expect_match("${ERR}" "unknown bridge command `frobnicate`")
rant(bridge status extra FAILS)
expect_match("${ERR}" "bridge status takes no arguments")
rant(bridge status --from x FAILS)
expect_match("${ERR}" "--from goes with `rant bridge enable`")
rant(bridge start -f FAILS)
expect_match("${ERR}" "--follow goes with `rant bridge logs`")
rant(bridge status --pass= FAILS)
expect_match("${ERR}" "--pass goes with `rant bridge enable`")
rant(bridge restart --bind 0.0.0.0 FAILS)
expect_match("${ERR}" "--bind goes with `rant bridge enable`")
