include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")
set(home "$ENV{RANT_HOME}")

cache_database("${RANT_LIBRARY}" [[{"name": "extra", "repo": "KosmosisDire/Rant-extra", "library": {"cmake": "extra::extra"}}]]
               [[{"name": "bridge", "repo": "KosmosisDire/rant-bridge"}]])

# Three packages, each naming Rant its own way, one taking another library, and a folder
# that uses none.
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
file(WRITE "${SCRATCH}/geo/CMakeLists.txt"
     "project(geo C)\nCPMAddPackage(NAME extra GIT_REPOSITORY https://github.com/KosmosisDire/Rant-extra.git GIT_TAG v1.2.0)\n")
file(WRITE "${SCRATCH}/tool/CMakeLists.txt" "project(tool C)\nadd_executable(tool tool.c)\n")

rant(ls libraries)
expect_match("${OUT}" "cam +rant +cmake +0[.]0[.]16 +CPM cam/CMakeLists[.]txt\n")
expect_match("${OUT}" "geo +extra +cmake +1[.]2[.]0 +CPM geo/CMakeLists[.]txt\n")
expect_match("${OUT}" "hmi +rant +csharp +0[.]0[.]14 +PackageReference hmi/hmi[.]csproj\n")
expect_match("${OUT}" "py +rant +python +0[.]0[.]15 +pyproject py/pyproject[.]toml\n")
expect_match("${OUT}" "py +rant +python +none +no venv yet\n")
expect_no_match("${OUT}" "tool")
rant(ls libraries extra)
expect_match("${OUT}" "^LIBRARIES\n  geo +extra +cmake +1[.]2[.]0 +CPM geo/CMakeLists[.]txt\n$")
rant(ls libraries --json)
expect_match("${OUT}" "\"library\": \"rant\",\n *\"folder\": [^\n]*cam\",\n *\"kind\": \"cmake\",\n *\"how\": \"CPM\"")
rant(ls libraries nothing)
expect_match("${ERR}" "nothing here uses a library yet")

rant(add rant nowhere FAILS)
expect_match("${ERR}" "no folder `nowhere`")
rant(add a b c FAILS)
expect_match("${ERR}" "add takes a library and one folder at most")
rant(add "Not a name" FAILS)
expect_match("${ERR}" "is no library name")

# A library is added and a package installed, each refusing the other, and a library comes
# only from a repo's releases.
file(WRITE "${SCRATCH}/geometry/package.rant" "name = \"geometry\"\nlibrary {\n  cmake = \"geometry\"\n}\n")
file(WRITE "${SCRATCH}/kit/package.rant" "name = \"kit\"\n")
file(WRITE "${home}/sources.rant" "paths = [\"${SCRATCH}/geometry\", \"${SCRATCH}/kit\"]\n")
rant(install geometry FAILS)
expect_match("${ERR}" "geometry is a library, add it to a package with `rant add geometry`")
rant(add kit FAILS)
expect_match("${ERR}" "kit is a package, install it with `rant install kit`")
rant(add geometry tool FAILS)
expect_match("${ERR}" "geometry is a folder in sources[.]rant, and a library comes from a repo's releases")

# The rest reads Rant's package.rant and its releases from GitHub, so it needs the network.
file(WRITE "${home}/sources.rant" "repos = [\"KosmosisDire/Rant\"]\n")
rant(add rant tool --version 0.0.17 MAY_FAIL)
if(NOT CODE EQUAL 0 AND ERR MATCHES "cannot reach|rate limit")
  message("SKIPPED: GitHub is not reachable: ${ERR}")
  return()
elseif(NOT CODE EQUAL 0)
  message(FATAL_ERROR "rant add rant tool exited ${CODE}\n${ERR}")
endif()
expect_match("${OUT}" "^rant 0[.]0[.]17\n  tool cmake: added to tool/CMakeLists[.]txt\n")
file(READ "${SCRATCH}/tool/CMakeLists.txt" tool)
expect_match("${tool}" "FetchContent_Declare[(]rant\n  GIT_REPOSITORY https://github.com/KosmosisDire/Rant[.]git\n  GIT_TAG v0[.]0[.]17\n")
expect_match("${tool}" "add_executable[(]tool tool[.]c[)]\ntarget_link_libraries[(]tool PRIVATE rant::rant_host[)]\n")

# add moves a library a folder names already, and only a version asked for moves it back.
rant(add rant cam --version 0.0.17)
expect_match("${OUT}" "cam cmake: 0[.]0[.]16 -> 0[.]0[.]17 in cam/CMakeLists[.]txt\n")
file(READ "${SCRATCH}/cam/CMakeLists.txt" cam)
expect_match("${cam}" "  GIT_TAG v0[.]0[.]17[)]")
rant(add rant cam --version 0.0.17)
expect_match("${ERR}" "cam uses rant 0[.]0[.]17 already")
rant(add --version 0.0.99 FAILS)
expect_match("${ERR}" "has no release 0[.]0[.]99")

# update moves every use in the workspace to the latest release, after listing it. The
# question is answered yes, since stdin is not a terminal.
file(WRITE "${SCRATCH}/ws/workspace.rant" "")
file(WRITE "${SCRATCH}/ws/arm/CMakeLists.txt" "project(arm C)\nCPMAddPackage(NAME rant GIT_REPOSITORY https://github.com/KosmosisDire/Rant.git GIT_TAG v0.0.17)\n")
rant(update rant IN "${SCRATCH}/ws")
expect_match("${OUT}" "^rant ([0-9.]+)\n  arm cmake: 0[.]0[.]17 -> [0-9.]+ in arm/CMakeLists[.]txt\n.*updated 1 package")
rant(update rant IN "${SCRATCH}/ws")
expect_match("${OUT}" "everything here is at [0-9.]+ or newer")

rant(ls libraries --available)
expect_match("${OUT}" "rant +KosmosisDire/Rant +cmake python csharp rust")

# A release that carries a skill gives it to the agents found, and remove takes it back.
file(MAKE_DIRECTORY "${SCRATCH}/home/.claude")
rant(add rant cam --version 0.0.40)
expect_match("${ERR}" "Also give the skill of rant to Claude Code")
file(READ "${SCRATCH}/home/.claude/skills/rant/SKILL.md" skill)
expect_match("${skill}" "name: rant")
rant(remove rant)
if(EXISTS "${SCRATCH}/home/.claude/skills/rant")
  message(FATAL_ERROR "rant remove left the skill of rant linked")
endif()
