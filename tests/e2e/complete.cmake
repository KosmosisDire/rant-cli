include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Shell startup files are written in the scratch home.
set(ENV{HOME} "${SCRATCH}/home")

# complete(<words before...> CUR <word at the cursor>) asks for completions the way a shell
# hook does and sets OUT to the candidates, one a line.
function(complete)
  cmake_parse_arguments(PARSE_ARGV 0 C "" "CUR" "")
  execute_process(COMMAND "${RANT}" __complete "--cur=${C_CUR}" --domain ${DOMAIN} ${C_UNPARSED_ARGUMENTS}
    WORKING_DIRECTORY "${CWD}" RESULT_VARIABLE code OUTPUT_VARIABLE out ERROR_VARIABLE err TIMEOUT 30)
  if(NOT code EQUAL 0)
    message(FATAL_ERROR "__complete exited ${code}\n${err}")
  endif()
  set(OUT "${out}" PARENT_SCOPE)
endfunction()

use_fixture(groups)
install_test_node(demo/bin)
file(WRITE "${SCRATCH}/opts.hcl" [[
group {
  param "target" {
    type    = string
    default = "red_bin"
    options = ["red_bin", "blue_bin"]
  }
  param "fast" {
    type    = bool
    default = false
  }
  node "x" {
    type = "demo/sensor"
  }
}
]])

complete(CUR st)
expect_match("${OUT}" "^start\nstop\n$")
complete(start CUR "")
expect_match("${OUT}" "^group\nnode\n$")
complete(start node CUR "")
expect_match("${OUT}" "^demo/sensor\ndemo/test_node\nsensor\ntest_node\n$")
complete(start group CUR "")
expect_match("${OUT}" "^base\nnav\nopts\npick\n$")

# A group's params end in = and give way to their options.
complete(start group nav CUR "")
expect_match("${OUT}" "^range=\nspeed=\n$")
complete(start group opts target=red_bin CUR "")
expect_match("${OUT}" "^fast=\n$")
complete(start group opts CUR target=)
expect_match("${OUT}" "^target=blue_bin\ntarget=red_bin\n$")
complete(start group opts CUR fast=t)
expect_match("${OUT}" "^fast=true\n$")

complete(build CUR "")
expect_match("${OUT}" "^demo\n$")
complete(build CUR --d)
expect_match("${OUT}" "^--domain\n--dry-run\n$")

# Nothing follows an option that takes a value, or a command that does not exist.
complete(--domain CUR "")
expect_match("${OUT}" "^$")
complete(nonsense CUR "")
expect_match("${OUT}" "^$")

# The mesh comes from what earlier commands saw, a path one segment at a time.
rant_beside_node("--name;cam;--pub;camera/left;--var;camera/rate" ls)
complete(sub CUR "")
expect_match("${OUT}" "^camera/\n$")
complete(sub CUR camera/)
expect_match("${OUT}" "^camera/left\n$")
complete(get CUR cam)
expect_match("${OUT}" "^camera/\n$")
complete(info CUR c)
expect_match("${OUT}" "^cam\ncamera/\n$")

# A full look at the mesh replaces the snapshot, so what left is no longer offered.
rant(ls)
complete(sub CUR "")
expect_match("${OUT}" "^$")

# setup writes the hook once, however often it runs.
rant(setup bash --print)
expect_match("${OUT}" "complete -F _rant_complete rant")
rant(setup bash)
rant(setup bash)
expect_match("${OUT}" "bash: tab completion loads from ")
file(READ "${SCRATCH}/home/.bashrc" bashrc)
string(REGEX MATCHALL "rant[.]bash" loads "${bashrc}")
list(LENGTH loads count)
if(NOT count EQUAL 2)
  message(FATAL_ERROR "expected one line loading the hook in .bashrc, got:\n${bashrc}")
endif()
if(NOT EXISTS "${SCRATCH}/home/.rant/completions/rant.bash")
  message(FATAL_ERROR "setup wrote no hook")
endif()
rant(setup tcsh FAILS)
expect_match("${ERR}" "unknown shell `tcsh`")
