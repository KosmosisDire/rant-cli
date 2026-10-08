# The config crate as a static library, plus the C header its build writes.

corrosion_import_crate(MANIFEST_PATH "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
  CRATES rant-config
  CRATE_TYPES staticlib)

set(RANT_CONFIG_INCLUDE "${CMAKE_CURRENT_BINARY_DIR}/include")
file(MAKE_DIRECTORY "${RANT_CONFIG_INCLUDE}")
corrosion_set_env_vars(rant_config "RANT_CONFIG_HEADER=${RANT_CONFIG_INCLUDE}/rant_config.h")
target_include_directories(rant_config INTERFACE "${RANT_CONFIG_INCLUDE}")

# Rust links the C runtime dynamically unless told, which clashes with the static runtime
# the rest of the build links on MSVC and Linux.
if((MSVC OR CMAKE_SYSTEM_NAME STREQUAL "Linux") AND RANT_STATIC_RUNTIME)
  corrosion_add_target_rustflags(rant_config "-Ctarget-feature=+crt-static")
endif()

# Corrosion asks for the shared libgcc_s, which a static link has no copy of. gcc links its
# static unwinder in its place.
if(CMAKE_SYSTEM_NAME STREQUAL "Linux" AND RANT_STATIC_RUNTIME)
  get_target_property(_native rant_config-static INTERFACE_LINK_LIBRARIES)
  list(REMOVE_ITEM _native gcc_s)
  set_property(TARGET rant_config-static PROPERTY INTERFACE_LINK_LIBRARIES ${_native})
endif()

# The crate's own unit tests, run by ctest beside the C++ ones.
if(RANT_CLI_TESTS)
  add_test(NAME config.rust
    COMMAND "${Rust_CARGO_CACHED}" test --lib --quiet --manifest-path "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
            --target-dir "${CMAKE_CURRENT_BINARY_DIR}/test-target")
  # The same compiler the build used, never whatever rustc the PATH finds first.
  set_tests_properties(config.rust PROPERTIES ENVIRONMENT "RUSTC=${Rust_COMPILER_CACHED}")
endif()
