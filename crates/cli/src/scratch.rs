//! A scratch folder for unit tests, removed with everything in it.

use std::path::PathBuf;

pub struct Scratch(pub PathBuf);

impl Scratch {
    /// An empty folder, named after the test so tests running at once never share one.
    pub fn new(name: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!("rant-unit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Scratch(root)
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    pub fn write(&self, rel: &str, text: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
