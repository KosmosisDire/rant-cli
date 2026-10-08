use std::fmt;
use std::path::{Path, PathBuf};

/// One located error. Line and column are one based, 0 means the whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl Diag {
    pub fn new(file: &Path, line: u32, column: u32, message: impl Into<String>) -> Diag {
        Diag { file: file.to_path_buf(), line, column, message: message.into() }
    }

    /// An error with no place in a file, such as a bad name on the command line.
    pub fn plain(message: impl Into<String>) -> Diag {
        Diag::new(Path::new(""), 0, 0, message)
    }

    /// An error about a whole file, such as one that cannot be read.
    pub fn file(file: &Path, message: impl Into<String>) -> Diag {
        Diag::new(file, 0, 0, message)
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.file.as_os_str().is_empty() {
            write!(f, "{}", self.message)
        } else if self.line == 0 {
            write!(f, "{}: {}", self.file.display(), self.message)
        } else {
            write!(f, "{}:{}:{}: {}", self.file.display(), self.line, self.column, self.message)
        }
    }
}
