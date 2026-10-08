# Three kinds of tests, all run by ctest:
#   config.rust   the config crate's own unit tests, through cargo (config/CMakeLists.txt)
#   unit.<name>   one C++ executable per module, tests/unit/<name>.cpp
#   e2e.<name>    one CMake script per behavior, driving the built rant, tests/e2e/<name>.cmake

# Test executables stay in the build tree, out of bin/.
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/bin")
foreach(_cfg ${CMAKE_CONFIGURATION_TYPES})
  string(TOUPPER "${_cfg}" _CFG)
  set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_${_CFG} "${CMAKE_CURRENT_BINARY_DIR}/bin")
endforeach()

add_subdirectory(nodes)
add_subdirectory(unit)
add_subdirectory(e2e)
