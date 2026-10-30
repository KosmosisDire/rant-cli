# Helpers for the end to end tests. Each test is a CMake script run by ctest through
# cmake -P, so it needs no shell and runs the same on every OS. tests/e2e/CMakeLists.txt
# passes RANT (the CLI), TEST_NODE (a small Rant node), FIXTURES, SCRATCH and DOMAIN.

cmake_minimum_required(VERSION 3.21)

# OTHER is a second workspace beside the scratch one, for a test that needs a node another
# workspace started.
set(OTHER "${SCRATCH}-other")

# Shell files and the skill folders of AI coding agents hang off the home, so no test reaches the
# real one. Toolchains keep theirs, which on Windows they find through USERPROFILE instead.
if(UNIX)
  set(tools CARGO_HOME RUSTUP_HOME NUGET_PACKAGES DOTNET_CLI_HOME PYTHONUSERBASE)
  set(tool_dirs .cargo .rustup .nuget/packages . .local)
  foreach(tool dir IN ZIP_LISTS tools tool_dirs)
    if(NOT DEFINED ENV{${tool}})
      set(ENV{${tool}} "$ENV{HOME}/${dir}")
    endif()
  endforeach()
endif()
set(ENV{HOME} "${SCRATCH}/home")
set(ENV{XDG_CONFIG_HOME} "${SCRATCH}/home/.config")

# A run that failed may have left nodes running in its scratch workspaces or its scratch
# home. They are stopped through the CLI first, since a running executable cannot be replaced.
set(ENV{RANT_HOME} "${SCRATCH}/home/.rant")
file(GLOB_RECURSE leftover_states "${SCRATCH}/.rant/state" "${SCRATCH}/*/.rant/state" "${OTHER}/.rant/state")
foreach(state_file IN LISTS leftover_states)
  get_filename_component(workspace "${state_file}" DIRECTORY)
  get_filename_component(workspace "${workspace}" DIRECTORY)
  file(READ "${state_file}" state_text)
  string(JSON count ERROR_VARIABLE json_error LENGTH "${state_text}" instances)
  if(json_error OR count EQUAL 0)
    continue()
  endif()
  math(EXPR last "${count} - 1")
  foreach(i RANGE ${last})
    string(JSON name GET "${state_text}" instances ${i} name)
    execute_process(COMMAND "${RANT}" stop "${name}" -y WORKING_DIRECTORY "${workspace}"
                    OUTPUT_QUIET ERROR_QUIET TIMEOUT 30)
  endforeach()
endforeach()

# A service process left behind ends once it has nothing to watch, the last line of its log.
set(services_log "$ENV{RANT_HOME}/logs/services.log")
foreach(i RANGE 40)
  if(NOT EXISTS "${services_log}")
    break()
  endif()
  file(READ "${services_log}" services_text)
  if(services_text MATCHES "nothing to watch\n$")
    break()
  endif()
  execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 0.5)
endforeach()

# Every run starts from an empty scratch directory, the working directory of the commands.
# Mesh snapshots and shell hooks go there too, never to the real home.
file(REMOVE_RECURSE "${SCRATCH}" "${OTHER}")
file(MAKE_DIRECTORY "${SCRATCH}")

# cache_database(<entry>...) writes what the cache keeps of the package database once read,
# so showing and adding a library reads no database. By default the database names nothing.
set(RANT_LIBRARY [[{"name": "rant", "repo": "KosmosisDire/Rant", "library": {"cmake": "rant::rant_host", "python": "rant-middleware", "csharp": "Rant", "rust": "rant-middleware"}}]])
function(cache_database)
  string(JOIN ", " entries ${ARGN})
  file(WRITE "$ENV{RANT_HOME}/cache/sources/KosmosisDire+rant-packages.json" "[${entries}]\n")
endfunction()
cache_database()
set(CWD "${SCRATCH}")
file(WRITE "${SCRATCH}/.empty-stdin" "")

# Checks one CLI run's exit code against FAILS and hands its output to the caller's caller
# as OUT and ERR, with Windows line ends made plain.
macro(finish_run code out err fails shown)
  if(${fails} AND ${code} EQUAL 0)
    message(FATAL_ERROR "rant ${shown} should have failed\nstdout:\n${${out}}\nstderr:\n${${err}}")
  elseif(NOT ${fails} AND NOT ${code} EQUAL 0)
    message(FATAL_ERROR "rant ${shown} exited ${${code}}\nstdout:\n${${out}}\nstderr:\n${${err}}")
  endif()
  string(REPLACE "\r\n" "\n" ${out} "${${out}}")
  string(REPLACE "\r\n" "\n" ${err} "${${err}}")
  set(OUT "${${out}}" PARENT_SCOPE)
  set(ERR "${${err}}" PARENT_SCOPE)
endmacro()

# rant(<args...> [FAILS | MAY_FAIL] [IN <dir>]) runs the CLI on the test's own domain with an
# empty, non terminal stdin. Nodes it starts inherit TEST_NODE_DOMAIN, so the test node joins
# the same domain. Sets OUT, ERR and CODE. The test fails unless the exit code is 0, or
# nonzero when FAILS is given. MAY_FAIL takes any exit code.
function(rant)
  cmake_parse_arguments(PARSE_ARGV 0 R "FAILS;MAY_FAIL" "IN" "")
  set(dir "${CWD}")
  if(R_IN)
    set(dir "${R_IN}")
  endif()
  execute_process(COMMAND "${CMAKE_COMMAND}" -E env "TEST_NODE_DOMAIN=${DOMAIN}"
                          "${RANT}" --domain ${DOMAIN} ${R_UNPARSED_ARGUMENTS}
    WORKING_DIRECTORY "${dir}"
    INPUT_FILE "${SCRATCH}/.empty-stdin"
    RESULT_VARIABLE code OUTPUT_VARIABLE out ERROR_VARIABLE err
    TIMEOUT 300)
  string(JOIN " " shown ${R_UNPARSED_ARGUMENTS})
  if(R_MAY_FAIL)
    set(R_FAILS OFF)
    if(NOT code EQUAL 0)
      set(R_FAILS ON)
    endif()
  endif()
  set(CODE "${code}" PARENT_SCOPE)
  finish_run(code out err R_FAILS "${shown}")
endfunction()

# rant_beside_node(<node args> <rant args...>) runs the CLI while a test node with the given
# ;-list of arguments runs beside it for a few seconds. Both start together, so the CLI meets
# the node the way it meets any node that just came up. Sets OUT and ERR.
function(rant_beside_node node_args)
  execute_process(
    COMMAND "${TEST_NODE}" --domain ${DOMAIN} --life 4 ${node_args}
    COMMAND "${RANT}" --domain ${DOMAIN} ${ARGN}
    WORKING_DIRECTORY "${CWD}"
    RESULTS_VARIABLE codes OUTPUT_VARIABLE out ERROR_VARIABLE err
    TIMEOUT 60)
  list(GET codes 1 code)
  set(no OFF)
  string(JOIN " " shown ${ARGN})
  finish_run(code out err no "${shown}")
endfunction()

function(expect_match text regex)
  if(NOT text MATCHES "${regex}")
    message(FATAL_ERROR "expected a match for: ${regex}\nin:\n${text}")
  endif()
endfunction()

function(expect_no_match text regex)
  if(text MATCHES "${regex}")
    message(FATAL_ERROR "expected no match for: ${regex}\nin:\n${text}")
  endif()
endfunction()

# Copies tests/e2e/fixtures/<name> into the scratch directory.
function(use_fixture name)
  file(COPY "${FIXTURES}/${name}/" DESTINATION "${SCRATCH}")
endfunction()

# install_test_node(<dir> [name]) copies the test node into the scratch directory as
# <dir>/<name>, test_node by default, with .exe on Windows.
function(install_test_node dir)
  set(name test_node)
  if(ARGC GREATER 1)
    set(name "${ARGV1}")
  endif()
  get_filename_component(ext "${TEST_NODE}" LAST_EXT)
  if(NOT ext STREQUAL ".exe")
    set(ext "")
  endif()
  file(MAKE_DIRECTORY "${SCRATCH}/${dir}")
  file(COPY_FILE "${TEST_NODE}" "${SCRATCH}/${dir}/${name}${ext}")
endfunction()

# wait_for_service_log(<regex>) waits up to 30 s for the service process's log to match and
# sets LOG to it.
function(wait_for_service_log regex)
  set(log "$ENV{RANT_HOME}/logs/services.log")
  set(text "")
  foreach(i RANGE 60)
    if(EXISTS "${log}")
      file(READ "${log}" text)
      if(text MATCHES "${regex}")
        set(LOG "${text}" PARENT_SCOPE)
        return()
      endif()
    endif()
    execute_process(COMMAND "${CMAKE_COMMAND}" -E sleep 0.5)
  endforeach()
  message(FATAL_ERROR "the service process's log never matched ${regex}:\n${text}")
endfunction()
