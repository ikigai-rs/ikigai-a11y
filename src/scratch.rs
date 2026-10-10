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
use std::sync::atomic::{AtomicU64, Ordering};

/// A directory under the system temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A fresh, empty config home, named for the test that owns it.
    pub(crate) fn new(tag: &str) -> Self {
        // Nanos alone are not unique: the clock ticks coarser than its unit
        // (microseconds on macOS), so two tests starting together read the same
        // value, and tags repeat. The counter is what disambiguates (ledger #163).
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ikigai-a11y-{}-{}-{}-{tag}",
            std::process::id(),
            // A host-side read, outside any resolution: this is the test harness
            // naming a directory, not an endpoint asking what time it is. The
            // kernel clock (`Invocation::now`) is for the latter.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after the epoch")
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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

#[cfg(test)]
mod tests {
    use super::Scratch;
    use std::sync::{Arc, Barrier};

    #[test]
    fn two_homes_with_one_tag_never_share_a_directory() {
        // `"empty"`, `"halves"` and `"layered"` each name two tests' homes in
        // this binary, which the harness runs on parallel threads, and each
        // home's `Drop` removes its directory: two that shared one would delete
        // each other's layers mid-test. So: threads released together, many
        // times over, all homes held until the end.
        const THREADS: usize = 8;
        let mut homes = Vec::new();
        for _ in 0..50 {
            let gate = Arc::new(Barrier::new(THREADS));
            let round: Vec<_> = (0..THREADS)
                .map(|_| {
                    let gate = Arc::clone(&gate);
                    std::thread::spawn(move || {
                        gate.wait();
                        Scratch::new("empty")
                    })
                })
                .collect();
            homes.extend(round.into_iter().map(|t| t.join().expect("thread")));
        }
        let distinct: std::collections::HashSet<&std::path::Path> =
            homes.iter().map(Scratch::path).collect();
        assert_eq!(distinct.len(), homes.len(), "scratch homes collided");
    }
}
