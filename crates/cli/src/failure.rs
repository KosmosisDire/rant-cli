//! Why a command failed, and the exit code that says so.

use rant_config::Diag;

#[derive(Debug)]
pub enum Failure {
    /// A command line rant cannot make sense of, exit code 2.
    Usage(String),
    /// What went wrong, exit code 1.
    Failed(String),
    /// Config errors, each at its place, exit code 1.
    Diags(Vec<Diag>),
}

pub type Result<T> = std::result::Result<T, Failure>;

impl Failure {
    pub fn code(&self) -> i32 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Failed(_) | Failure::Diags(_) => 1,
        }
    }

    /// Each line an error prints, without the `error:` in front.
    pub fn messages(&self) -> Vec<String> {
        match self {
            Failure::Usage(m) | Failure::Failed(m) => vec![m.clone()],
            Failure::Diags(d) => d.iter().map(Diag::to_string).collect(),
        }
    }
}

impl From<Diag> for Failure {
    fn from(d: Diag) -> Failure {
        Failure::Diags(vec![d])
    }
}

pub fn usage(message: impl Into<String>) -> Failure {
    Failure::Usage(message.into())
}

pub fn failed(message: impl Into<String>) -> Failure {
    Failure::Failed(message.into())
}
