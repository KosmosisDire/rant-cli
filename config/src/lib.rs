//! The config layer of the rant CLI: finding the workspace, its packages and their node
//! types, parsing and evaluating its HCL, and handing the C++ side resolved data through
//! the C ABI in `ffi`.

mod argv;
mod build_plan;
mod diag;
mod discover;
mod eval;
pub mod ffi;
pub mod ffi_lib;
mod group;
mod lib_use;
mod manifest;
mod model;
mod package;
mod param;
mod paths;
mod plan;
mod refs;
mod scan;
mod template;
mod source;
mod workspace;

#[cfg(test)]
mod testdir;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod group_tests;
