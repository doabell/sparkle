use std::path::{Path, PathBuf};

/// Owns only a uniquely created test directory; cleanup also runs on assertion failure.
pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(crate::analytics::new_trace_id("sparkle-test"));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub(crate) fn audio(&self, name: &str) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, include_bytes!("fixtures/tone.flac")).unwrap();
        path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
