include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

rant(explore FAILS)
expect_match("${ERR}" "the explorer is not installed, `rant install explorer` installs it")

# A stand in explorer as rant install leaves one: the CLI itself, which prints its help for
# `--domain N` and exits, so nothing stays open and no network is needed.
get_filename_component(ext "${RANT}" LAST_EXT)
if(NOT ext STREQUAL ".exe")
  set(ext "")
endif()
set(pkg "${SCRATCH}/home/.rant/packages/explorer")
file(MAKE_DIRECTORY "${pkg}/bin")
file(COPY_FILE "${RANT}" "${pkg}/bin/rant_explorer${ext}")
file(WRITE "${pkg}/package.rant" "node \"explorer\" {\n  run = \"$\{path.package}/bin/rant_explorer\"\n}\n")

rant(explore)
expect_match("${ERR}" "started .*/home/[.]rant/packages/explorer/bin/rant_explorer")

# The explorer runs detached, so its output reaches the log a moment later.
set(log "${SCRATCH}/home/.rant/explorer.log")
foreach(i RANGE 50)
  if(EXISTS "${log}")
    file(READ "${log}" text)
    if(text MATCHES "Usage: rant")
      break()
    endif()
  endif()
  execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 0.1)
endforeach()
expect_match("${text}" "rant: build, run and inspect Rant nodes")

rant(explore extra FAILS)
expect_match("${ERR}" "explore takes no arguments")

# explorer is another name for explore.
rant(explorer --help)
expect_match("${OUT}" "Usage: rant explore")
