include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# A stand in explorer where rant installs one: the CLI itself, which prints its help for
# `--domain N` and exits, so nothing stays open and no network is needed.
get_filename_component(ext "${RANT}" LAST_EXT)
if(NOT ext STREQUAL ".exe")
  set(ext "")
endif()
file(MAKE_DIRECTORY "${SCRATCH}/home/.rant/bin")
file(COPY_FILE "${RANT}" "${SCRATCH}/home/.rant/bin/rant-explorer${ext}")

rant(explore)
expect_match("${ERR}" "started .*/home/[.]rant/bin/rant-explorer")

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
expect_match("${ERR}" "explore takes no words")
