# A CMake package that compiles nothing, so the test needs no compiler. Mentioning Rant
# is what makes it a package, and find_package(sdk) is its dependency on the sdk package.
cmake_minimum_required(VERSION 3.21)
project(driver NONE)
find_package(rant QUIET)
find_package(sdk QUIET)
add_custom_target(driver ALL COMMAND "${CMAKE_COMMAND}" -E echo building-driver)
