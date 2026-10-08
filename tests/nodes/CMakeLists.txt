# Small Rant programs the end to end tests run as nodes.
add_executable(test_node test_node.cpp)
target_link_libraries(test_node PRIVATE rant_impl)
