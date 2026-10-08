cmake_minimum_required(VERSION 3.21)
project({{name}} CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_executable({{name}} {{name}}.cpp)
target_link_libraries({{name}} PRIVATE rant::rant_host)
