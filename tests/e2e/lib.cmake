# Helpers for the end to end tests. Each test is a CMake script run by ctest through
# cmake -P, so it needs no shell and runs the same on every OS. tests/e2e/CMakeLists.txt
# passes RANT (the CLI), TEST_NODE (a small Rant node), FIXTURES, SCRATCH and DOMAIN.

cmake_minimum_required(VERSION 3.21)

# Every run starts from an empty scratch directory, the working directory of the commands.
file(REMOVE_RECURSE "${SCRATCH}")
file(MAKE_DIRECTORY "${SCRATCH}")
set(CWD "${SCRATCH}")
file(WRITE "${SCRATCH}/.empty-stdin" "")

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
  if(R_FAILS AND code EQUAL 0)
    message(FATAL_ERROR "rant ${shown} should have failed\nstdout:\n${out}\nstderr:\n${err}")
  elseif(NOT R_FAILS AND NOT code EQUAL 0)
    message(FATAL_ERROR "rant ${shown} exited ${code}\nstdout:\n${out}\nstderr:\n${err}")
  endif()
  set(OUT "${out}" PARENT_SCOPE)
  set(ERR "${err}" PARENT_SCOPE)
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
