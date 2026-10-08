template {
  description = "One C++ node source file"
  next        = "add it to CMakeLists.txt:\n  add_executable({{ name }} {{ name }}.cpp)\n  target_link_libraries({{ name }} PRIVATE rant::rant_host)"
}
