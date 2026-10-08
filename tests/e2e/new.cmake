include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

rant(init)

rant(new group nav)
expect_match("${OUT}" "created nav[.]group[.]hcl")
file(READ "${SCRATCH}/nav.group.hcl" nav)
expect_match("${nav}" "description = \"What nav runs\"")
rant(new group nav FAILS)
expect_match("${ERR}" "nav[.]group[.]hcl exists already, nothing was written")

# With no name a node or group takes the name of the folder it is made in.
file(MAKE_DIRECTORY "${SCRATCH}/lidar")
rant(new group IN "${SCRATCH}/lidar")
expect_match("${OUT}" "created lidar[.]group[.]hcl")

# A node takes the language of the package it is made in.
file(WRITE "${SCRATCH}/py/pyproject.toml" "[project]\nname = \"py\"\ndependencies = [\"rant-middleware\"]\n")
rant(new node camera IN "${SCRATCH}/py")
expect_match("${OUT}" "created camera[.]py")
rant(build --dry-run)
expect_match("${OUT}" "py/camera +python +py/camera[.]py")

rant(new node arm FAILS)
expect_match("${ERR}" "say which language with --lang")
rant(new node arm --lang rust FAILS)
expect_match("${ERR}" "unknown language `rust`")
rant(new thing x FAILS)
expect_match("${ERR}" "say what to make")

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

# A package gets Rant through the same routine as rant lib install, which asks GitHub.
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
rant(lib cam)
expect_match("${OUT}" "cam +cmake +[0-9]+[.][0-9]+[.][0-9]+ +FetchContent")
