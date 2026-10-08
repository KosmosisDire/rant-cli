# Helpers for the end to end tests. Each test is a CMake script run by ctest through
# cmake -P, so it needs no shell and runs the same on every OS. tests/e2e/CMakeLists.txt
# passes RANT (the CLI), TEST_NODE (a small Rant node), FIXTURES, SCRATCH and DOMAIN.

cmake_minimum_required(VERSION 3.21)

# Every run starts from an empty scratch directory, the working directory of the commands.
file(REMOVE_RECURSE "${SCRATCH}")
file(MAKE_DIRECTORY "${SCRATCH}")
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

# rant(<args...> [FAILS] [IN <dir>]) runs the CLI on the test's own domain with an empty,
# non terminal stdin. Sets OUT and ERR. The test fails unless the exit code is 0, or
# nonzero when FAILS is given.
function(rant)
  cmake_parse_arguments(PARSE_ARGV 0 R "FAILS" "IN" "")
  set(dir "${CWD}")
  if(R_IN)
    set(dir "${R_IN}")
  endif()
  execute_process(COMMAND "${RANT}" --domain ${DOMAIN} ${R_UNPARSED_ARGUMENTS}
    WORKING_DIRECTORY "${dir}"
    INPUT_FILE "${SCRATCH}/.empty-stdin"
    RESULT_VARIABLE code OUTPUT_VARIABLE out ERROR_VARIABLE err
    TIMEOUT 60)
  string(JOIN " " shown ${R_UNPARSED_ARGUMENTS})
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
