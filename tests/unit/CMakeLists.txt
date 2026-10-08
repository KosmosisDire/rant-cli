add_library(check_main STATIC check_main.cpp)

# unit_test(<name>) builds <name>.cpp against the CLI code as test_<name>, ctest unit.<name>.
function(unit_test name)
  add_executable(test_${name} ${name}.cpp)
  target_link_libraries(test_${name} PRIVATE rant_cli_core check_main)
  add_test(NAME unit.${name} COMMAND test_${name})
endfunction()

unit_test(args)
