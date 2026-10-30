//! The config layer of the rant CLI: finding the workspace, its packages and their node
//! types, parsing and evaluating its HCL, reading the package database and the files
//! packages install, and handing the C++ side resolved data through the C ABI in `ffi`.

mod argv;
mod build_plan;
mod database;
mod diag;
mod discover;
mod eval;
pub mod ffi;
mod group;
mod manifest;
mod model;
mod package;
mod param;
mod paths;
mod placement;
mod plan;
mod refs;
mod scan;
mod service;
mod source;
mod vsix;
mod workspace;

#[cfg(test)]
mod testdir;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod group_tests;
