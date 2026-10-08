# The rant command line tool: C++ for the CLI, the mesh and the processes, a Rust crate in
# config/ for the HCL config layer. CPM fetches Rant, Corrosion, nlohmann json and inja.

cmake_minimum_required(VERSION 3.21)

file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/VERSION" RANT_CLI_VERSION LIMIT_COUNT 1)
project(rant_cli VERSION ${RANT_CLI_VERSION} LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Release first so a bare multi config build is Release. Every config lands in bin/.
if(CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_CONFIGURATION_TYPES "Release;Debug;RelWithDebInfo;MinSizeRel"
      CACHE STRING "Build configs (Release first = the default)" FORCE)
endif()
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/bin")
foreach(_cfg ${CMAKE_CONFIGURATION_TYPES})
  string(TOUPPER "${_cfg}" _CFG)
  set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_${_CFG} "${CMAKE_CURRENT_SOURCE_DIR}/bin")
endforeach()

include("${CMAKE_CURRENT_SOURCE_DIR}/cmake/get_cpm.cmake")

# 0.0.18 is the first release with the binary magic that finds native nodes and the
# RANT_NODE_NAME a started node takes its name from. The local presets set
# CPM_rant_SOURCE to ../rant to build against the working tree instead.
CPMAddPackage(NAME rant
  GITHUB_REPOSITORY KosmosisDire/Rant
  VERSION 0.0.18
  EXCLUDE_FROM_ALL YES
  SYSTEM YES)

# Static CRT: a copied exe must run with no VC++ redistributable. Before every dependency,
# so they all build against the same runtime.
include("${rant_SOURCE_DIR}/tools/static_runtime.cmake")

# One Linux build for every distribution: glibc is linked in too, so the binary needs only
# the kernel. Safe because nothing here calls the NSS lookups that static glibc breaks.
if(CMAKE_SYSTEM_NAME STREQUAL "Linux" AND RANT_STATIC_RUNTIME)
  set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -static")
endif()

CPMAddPackage(NAME Corrosion
  GITHUB_REPOSITORY corrosion-rs/corrosion
  VERSION 0.5.2
  EXCLUDE_FROM_ALL YES)

CPMAddPackage(NAME nlohmann_json
  URL https://github.com/nlohmann/json/releases/download/v3.12.0/json.tar.xz
  VERSION 3.12.0
  EXCLUDE_FROM_ALL YES)

# Header only, over the nlohmann json above.
CPMAddPackage(NAME inja
  GITHUB_REPOSITORY pantor/inja
  VERSION 3.4.0
  DOWNLOAD_ONLY YES)
add_library(inja INTERFACE)
target_include_directories(inja SYSTEM INTERFACE "${inja_SOURCE_DIR}/include")
target_link_libraries(inja INTERFACE nlohmann_json::nlohmann_json)

option(RANT_CLI_TESTS "Build the tests" ${PROJECT_IS_TOP_LEVEL})
if(RANT_CLI_TESTS)
  enable_testing()
endif()

add_subdirectory(config)
add_subdirectory(src)

if(RANT_CLI_TESTS)
  add_subdirectory(tests)
endif()
