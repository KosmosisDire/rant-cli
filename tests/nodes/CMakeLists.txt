# Small Rant programs the end to end tests run as nodes.
add_executable(test_node test_node.cpp)
target_link_libraries(test_node PRIVATE rant_impl)

# A Windows GUI program with a hidden window, as the bridge is, which rant stops by closing it.
if(WIN32)
  add_executable(test_window WIN32 test_window.cpp)
  if(MSVC)
    target_link_options(test_window PRIVATE /ENTRY:mainCRTStartup)
  endif()
endif()
