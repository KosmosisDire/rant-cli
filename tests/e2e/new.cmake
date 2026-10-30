include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")
# Rant is known as a library, as it is once the package database was read.
cache_database("${RANT_LIBRARY}")

rant(init)

rant(new group nav)
expect_match("${OUT}" "created Nav[.]run")
file(READ "${SCRATCH}/Nav.run" nav)
expect_match("${nav}" "description = \"What Nav runs\"")
rant(new group nav FAILS)
expect_match("${ERR}" "Nav[.]run exists already, nothing was written")

# With no name a node or group takes the name of the folder it is made in.
file(MAKE_DIRECTORY "${SCRATCH}/lidar")
rant(new group IN "${SCRATCH}/lidar")
expect_match("${OUT}" "created Lidar[.]run")

# A node takes the language of the package it is made in.
file(WRITE "${SCRATCH}/py/pyproject.toml" "[project]\nname = \"py\"\ndependencies = [\"rant-middleware\"]\n")
rant(new node camera IN "${SCRATCH}/py")
expect_match("${OUT}" "created camera[.]py")
rant(build --dry-run)
expect_match("${OUT}" "py/camera +python +py/camera[.]py")

rant(new node arm FAILS)
expect_match("${ERR}" "say which language with --lang")
rant(new node arm --lang cobol FAILS)
expect_match("${ERR}" "unknown language `cobol`, use cpp, python, csharp or rust")
rant(new thing x FAILS)
expect_match("${ERR}" "say what to make")
rant(new workspace demo FAILS)
expect_match("${ERR}" "say which language with --lang")

# A C++ node joins the CMake project around it, and needs one.
rant(new node arm --lang cpp FAILS)
expect_match("${ERR}" "no CMakeLists.txt here or above to add `arm` to")
file(WRITE "${SCRATCH}/robot/CMakeLists.txt" "project(robot CXX)\nadd_executable(base base.cpp)\n")
file(MAKE_DIRECTORY "${SCRATCH}/robot/nodes")
rant(new node arm IN "${SCRATCH}/robot/nodes" MAY_FAIL)
expect_match("${OUT}" "created arm[.]cpp\nadded arm to [.][.]/CMakeLists[.]txt\n")
file(READ "${SCRATCH}/robot/CMakeLists.txt" robot)
expect_match("${robot}" "add_executable[(]base base[.]cpp[)]\nadd_executable[(]arm nodes/arm[.]cpp[)]\ntarget_link_libraries[(]arm PRIVATE rant::rant_host[)]\n")
rant(new node arm --lang cpp IN "${SCRATCH}/robot" FAILS)
expect_match("${ERR}" "target `arm` already, nothing was written")

# A Rust node is a program file of the Cargo package around it, and needs one.
find_program(CARGO cargo)
if(CARGO)
  rant(new node motor --lang rust FAILS)
  expect_match("${ERR}" "no Cargo package here or above to add `motor` to")
  file(WRITE "${SCRATCH}/drive/Cargo.toml" "[package]
name = \"drive\"

[dependencies]
rant-middleware = \"0.0.24\"
")
  file(MAKE_DIRECTORY "${SCRATCH}/drive/src")
  rant(new node motor IN "${SCRATCH}/drive/src")
  expect_match("${OUT}" "created bin/motor[.]rs
")
  rant(build --dry-run)
  expect_match("${OUT}" "drive/motor +rust +drive/src/bin/motor[.]rs")
  rant(new workspace rusty --lang rust MAY_FAIL)
  expect_match("${OUT}" "created rusty/src/bin/listener[.]rs
created rusty/src/bin/talker[.]rs
created rusty/workspace[.]rant
created rusty/Rusty[.]run
created rusty/[.]gitignore
created rusty/Cargo[.]toml
")
endif()

# A workspace holds a talker, a listener and a group of both. Its CMake package knows its
# nodes once built.
rant(new workspace demo --lang cpp MAY_FAIL)
expect_match("${OUT}" "created demo/CMakeLists[.]txt
created demo/src/listener[.]cpp
created demo/src/talker[.]cpp
created demo/workspace[.]rant
created demo/Demo[.]run
")
rant(run Demo --dry-run IN "${SCRATCH}/demo" FAILS)
expect_match("${ERR}" "no node named `talker`, `demo` is not built yet, run `rant build` first")

# A package gets Rant through the same routine as rant add, which asks GitHub for the release.
rant(new package cam --lang cpp MAY_FAIL)
expect_match("${OUT}" "created cam/CMakeLists[.]txt\ncreated cam/cam[.]cpp\n")
if(NOT CODE EQUAL 0 AND ERR MATCHES "cannot reach|rate limit")
  message("SKIPPED: GitHub is not reachable: ${ERR}")
  return()
elseif(NOT CODE EQUAL 0)
  message(FATAL_ERROR "rant new package cam exited ${CODE}\n${ERR}")
endif()
file(READ "${SCRATCH}/cam/CMakeLists.txt" cam)
expect_match("${cam}" "FetchContent_MakeAvailable[(]rant[)]")
expect_match("${cam}" "add_executable[(]cam cam[.]cpp[)]\ntarget_link_libraries[(]cam PRIVATE rant::rant_host[)]\n")
rant(ls libraries)
expect_match("${OUT}" "cam +rant +cmake +[0-9]+[.][0-9]+[.][0-9]+ +FetchContent")
