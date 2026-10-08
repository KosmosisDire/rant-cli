//! The config layer of the rant CLI: finding the workspace, parsing and evaluating its HCL,
//! and handing the C++ side resolved data through the C ABI in `ffi`.

mod diag;
mod eval;
pub mod ffi;
mod source;
mod workspace;

#[cfg(test)]
mod testdir;
