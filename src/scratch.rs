//! A scratch config home for tests: a directory that writes the layers a test
//! wants to assert about and removes itself afterwards.
//!
//! Shared by [`crate::load`]'s tests and [`crate::endpoints`]'s, because after
//! the endpoints started taking their config home the two need exactly the same
//! fixture — one of them building its own would be the first step of a drift.
//!
//! Two directories are not worth a dev-dependency, and `std::env::temp_dir()`
//! is a real temp directory on every platform CI runs on.

use std::path::{Path, PathBuf};

/// A directory under the system temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A fresh, empty config home, named for the test that owns it.
    pub(crate) fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ikigai-a11y-{}-{}-{tag}",
            std::process::id(),
            // A host-side read, outside any resolution: this is the test harness
            // naming a directory, not an endpoint asking what time it is. The
            // kernel clock (`Invocation::now`) is for the latter.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    /// Write one layer file into this home.
    pub(crate) fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).expect("scratch write");
    }

    /// The config home itself — what a handle or a `*_in` call is given.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
