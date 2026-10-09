cmake_minimum_required(VERSION 3.21)
project({{name}} CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_executable(talker talker.cpp)
add_executable(listener listener.cpp)
target_link_libraries(talker PRIVATE rant::rant_host)
target_link_libraries(listener PRIVATE rant::rant_host)
