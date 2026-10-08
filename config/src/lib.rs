//! The config layer of the rant CLI: finding the workspace, its packages and their node
//! types, parsing and evaluating its HCL, and handing the C++ side resolved data through
//! the C ABI in `ffi`.

mod argv;
mod diag;
mod discover;
mod eval;
pub mod ffi;
mod manifest;
mod model;
mod package;
mod paths;
mod refs;
mod scan;
mod source;
mod workspace;

#[cfg(test)]
mod testdir;
#[cfg(test)]
mod tests;
