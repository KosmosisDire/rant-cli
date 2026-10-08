include("${CMAKE_CURRENT_LIST_DIR}/lib.cmake")

find_program(DOTNET dotnet)
if(NOT DOTNET)
  message("SKIPPED: dotnet is not installed")
  return()
endif()

use_fixture(csharp)

# The C# program is found from its project, and builds with dotnet build.
rant(build --dry-run)
expect_match("${OUT}" "panel/panel +csharp +panel/panel.csproj")
expect_match("${OUT}" "1. panel\n +dotnet build .*/panel/panel.csproj\n")

# It cannot start before it is built.
rant(start node panel FAILS)
expect_match("${ERR}" "`panel/panel` is not built yet, run `rant build panel` first")

# Built, it is found, started, listed and stopped like any other node.
rant(build)
expect_match("${OUT}" "built 1 package")
rant(build --dry-run)
expect_match("${OUT}" "panel/panel +csharp +panel/bin/[^\n]*/panel")
rant(start node panel)
expect_match("${OUT}" "^started panel,")
rant(ls nodes)
expect_match("${OUT}" "NODES\n  panel\n")
rant(stop node panel)
expect_match("${OUT}" "^stopped panel\n")
file(READ "${SCRATCH}/logs/panel.log" log)
expect_match("${log}" "ready panel")
