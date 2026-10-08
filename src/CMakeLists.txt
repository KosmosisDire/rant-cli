# Everything but main() is one static library, so the unit tests link the same code.

# The one RANT_IMPLEMENTATION anchor, on its own so the warnings below apply to our code only.
add_library(rant_impl STATIC "${rant_SOURCE_DIR}/dist/rant.cpp")
target_link_libraries(rant_impl PUBLIC rant::rant rant::rant_platform)

add_library(rant_cli_core STATIC
  app/args.cpp
  app/context.cpp
  app/run.cpp
  commands/build.cpp
  commands/call.cpp
  commands/commands.cpp
  commands/format.cpp
  commands/get.cpp
  commands/info.cpp
  commands/init.cpp
  commands/lib.cpp
  commands/ls.cpp
  commands/new.cpp
  commands/plan_output.cpp
  commands/pub.cpp
  commands/set.cpp
  commands/setup.cpp
  commands/start.cpp
  commands/stop.cpp
  commands/sub.cpp
  complete/complete.cpp
  complete/shells.cpp
  config/config.cpp
  config/lib.cpp
  library/library.cpp
  mesh/access.cpp
  mesh/client.cpp
  mesh/encode.cpp
  mesh/schema_text.cpp
  mesh/snapshot.cpp
  mesh/text.cpp
  mesh/values.cpp
  process/command.cpp
  process/quote.cpp
  run/nodes.cpp
  state/state.cpp
  ui/csv.cpp
  ui/live.cpp
  ui/output.cpp
  ui/prompt.cpp
  ui/table.cpp
  ui/terminal.cpp
  ui/yaml.cpp
  util/home.cpp
  util/interrupt.cpp
  util/match.cpp)

# One process backend per OS behind process/supervisor.hpp.
if(WIN32)
  target_sources(rant_cli_core PRIVATE process/windows.cpp)
else()
  target_sources(rant_cli_core PRIVATE process/posix.cpp)
endif()

target_include_directories(rant_cli_core PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
target_compile_definitions(rant_cli_core PUBLIC RANT_CLI_VERSION="${PROJECT_VERSION}")
target_link_libraries(rant_cli_core PUBLIC
  rant_impl rant_config nlohmann_json::nlohmann_json)

if(MSVC)
  target_compile_options(rant_cli_core PUBLIC /W4 /utf-8)
  target_compile_definitions(rant_cli_core PUBLIC _CRT_SECURE_NO_WARNINGS)
else()
  target_compile_options(rant_cli_core PUBLIC -Wall -Wextra)
endif()

add_executable(rant_cli main.cpp)
set_target_properties(rant_cli PROPERTIES OUTPUT_NAME rant)
target_link_libraries(rant_cli PRIVATE rant_cli_core)
