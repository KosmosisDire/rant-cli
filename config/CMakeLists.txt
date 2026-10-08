# The config crate as a static library, plus the C header its build writes.

corrosion_import_crate(MANIFEST_PATH "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
  CRATES rant-config
  CRATE_TYPES staticlib)

set(RANT_CONFIG_INCLUDE "${CMAKE_CURRENT_BINARY_DIR}/include")
file(MAKE_DIRECTORY "${RANT_CONFIG_INCLUDE}")
corrosion_set_env_vars(rant_config "RANT_CONFIG_HEADER=${RANT_CONFIG_INCLUDE}/rant_config.h")
target_include_directories(rant_config INTERFACE "${RANT_CONFIG_INCLUDE}")

# Rust links the dynamic CRT on MSVC unless told, which clashes with the static one above.
if(MSVC AND RANT_STATIC_RUNTIME)
  corrosion_add_target_rustflags(rant_config "-Ctarget-feature=+crt-static")
endif()

# The crate's own unit tests, run by ctest beside the C++ ones.
if(RANT_CLI_TESTS)
  add_test(NAME config.rust
    COMMAND "${Rust_CARGO_CACHED}" test --lib --quiet --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
            --target-dir "${CMAKE_CURRENT_BINARY_DIR}/test-target")
  # The same compiler the build used, never whatever rustc the PATH finds first.
  set_tests_properties(config.rust PROPERTIES ENVIRONMENT "RUSTC=${Rust_COMPILER_CACHED}")
endif()
