//! The config files of rant-cli: workspaces, their packages, node types, groups and services,
//! and the package database. `Workspace` is where commands start.

mod argv;
pub mod build_plan;
pub mod database;
mod diag;
pub mod discover;
mod eval;
mod filter;
pub mod group;
pub mod manifest;
pub mod model;
pub mod package;
pub mod param;
pub mod paths;
pub mod placement;
pub mod plan;
pub mod python;
pub mod refs;
pub mod scan;
pub mod service;
pub mod settings;
mod source;
mod workspace;

#[cfg(test)]
mod testdir;

pub use diag::Diag;
pub use hcl::Value;
pub use settings::Settings;
pub use workspace::{GroupInfo, GroupName, GroupPlan, ServiceCheck, Workspace};
