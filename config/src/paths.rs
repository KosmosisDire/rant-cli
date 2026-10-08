//! Path comparison that agrees with the OS: lexical cleanup, and no case on Windows.

use std::path::{Component, Path, PathBuf};

/// Absolute, with `.` and `..` resolved by text alone so links are never followed.
pub fn normalize(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

fn key(p: &Path) -> String {
    let s = normalize(p).to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

pub fn same(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// True when `child` is `parent` or lies below it.
pub fn within(child: &Path, parent: &Path) -> bool {
    let (c, p) = (key(child), key(parent));
    c == p || c.starts_with(&format!("{}/", p.trim_end_matches('/')))
}

/// `p` relative to `base` with forward slashes, for globs and display.
pub fn relative(p: &Path, base: &Path) -> String {
    let n = normalize(p);
    let b = normalize(base);
    n.strip_prefix(&b).unwrap_or(&n).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_cleanup_and_containment() {
        let base = std::env::temp_dir();
        assert!(same(&base.join("a/./b/../c"), &base.join("a/c")));
        assert!(within(&base.join("a/c/d"), &base.join("a/c")));
        assert!(!within(&base.join("a/cd"), &base.join("a/c")));
        assert_eq!(relative(&base.join("a/c/d.txt"), &base.join("a")), "c/d.txt");
    }
}
