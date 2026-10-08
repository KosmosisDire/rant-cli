include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

# Three packages, each naming Rant its own way, and a folder that does not use it.
file(WRITE "${SCRATCH}/cam/CMakeLists.txt" [[
cmake_minimum_required(VERSION 3.21)
project(cam CXX)
include(cmake/get_cpm.cmake)
CPMAddPackage(
  NAME rant
  GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git
  GIT_TAG v0.0.16)
add_executable(cam main.cpp)
]])
file(WRITE "${SCRATCH}/py/pyproject.toml" "[project]\nname = \"py\"\ndependencies = [\"rant-middleware>=0.0.15\"]\n")
file(WRITE "${SCRATCH}/hmi/hmi.csproj" "<Project>\n  <ItemGroup>\n    <PackageReference Include=\"Rant\" Version=\"0.0.14\" />\n  </ItemGroup>\n</Project>\n")
file(WRITE "${SCRATCH}/tool/CMakeLists.txt" "project(tool C)\nadd_executable(tool tool.c)\n")

rant(lib)
expect_match("${OUT}" "cam +cmake +0[.]0[.]16 +CPM cam/CMakeLists[.]txt\n")
expect_match("${OUT}" "hmi +csharp +0[.]0[.]14 +PackageReference hmi/hmi[.]csproj\n")
expect_match("${OUT}" "py +python +0[.]0[.]15 +pyproject py/pyproject[.]toml\n")
expect_match("${OUT}" "py +python +none +no venv yet\n")
expect_no_match("${OUT}" "tool")

rant(lib tool)
expect_match("${ERR}" "`tool` does not use Rant yet")
rant(lib --json)
expect_match("${OUT}" "\"how\": \"CPM\"")
rant(lib nowhere FAILS)
expect_match("${ERR}" "no folder `nowhere`")
rant(lib --version 0.0.17 FAILS)
expect_match("${ERR}" "--version goes with `rant lib install`")

# The rest asks GitHub for the release, so it needs the network.
rant(lib install tool --version 0.0.17 MAY_FAIL)
if(NOT CODE EQUAL 0 AND ERR MATCHES "cannot reach|rate limit")
  message("SKIPPED: GitHub is not reachable: ${ERR}")
  return()
elseif(NOT CODE EQUAL 0)
  message(FATAL_ERROR "rant lib install tool exited ${CODE}\n${ERR}")
endif()
file(READ "${SCRATCH}/tool/CMakeLists.txt" tool)
expect_match("${tool}" "FetchContent_Declare[(]rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant[.]git\n  GIT_TAG v0[.]0[.]17\n")
expect_match("${tool}" "add_executable[(]tool tool[.]c[)]\ntarget_link_libraries[(]tool PRIVATE rant::rant_host[)]\n")

rant(lib install cam --version 0.0.17)
expect_match("${OUT}" "cam cmake: 0[.]0[.]16 -> 0[.]0[.]17")
file(READ "${SCRATCH}/cam/CMakeLists.txt" cam)
expect_match("${cam}" "  GIT_TAG v0[.]0[.]17[)]")

rant(lib install cam --version 0.0.17)
expect_match("${OUT}" "cam cmake: already 0[.]0[.]17")

rant(lib install --version 0.0.99 FAILS)
expect_match("${ERR}" "has no release v0[.]0[.]99")
