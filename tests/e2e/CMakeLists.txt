# e2e_test(<name>) runs <name>.cmake against the built rant. Every test gets its own Rant
# domain, so tests run in parallel and never see a real mesh on the default domain.
set(E2E_NEXT_DOMAIN 140 CACHE INTERNAL "")

function(e2e_test name)
  add_test(NAME e2e.${name}
    COMMAND "${CMAKE_COMMAND}"
            "-DRANT=$<TARGET_FILE:rant_cli>"
            "-DTEST_NODE=$<TARGET_FILE:test_node>"
            "-DFIXTURES=${CMAKE_CURRENT_SOURCE_DIR}/fixtures"
            "-DSCRATCH=${CMAKE_CURRENT_BINARY_DIR}/scratch/${name}"
            "-DDOMAIN=${E2E_NEXT_DOMAIN}"
            -P "${CMAKE_CURRENT_SOURCE_DIR}/${name}.cmake")
  math(EXPR next "${E2E_NEXT_DOMAIN} + 1")
  set(E2E_NEXT_DOMAIN ${next} CACHE INTERNAL "")
endfunction()

e2e_test(init)
e2e_test(mesh_read)
e2e_test(discovery)
e2e_test(single_node)
e2e_test(groups)
