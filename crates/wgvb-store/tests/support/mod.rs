//! A scratch directory that cleans up after itself.
//!
//! Every test here needs a real file: `rusqlite` in-memory databases cannot be
//! closed and reopened, and reopening is half of what is being tested.

// Shared by several test binaries, and no one binary uses all of it. Cargo
// compiles this module into each of them separately, so what is unused in one
// is not dead code in the crate.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory that is removed when it goes out of scope.
pub struct Scratch(PathBuf);

impl Scratch {
    /// Makes a directory named for this process, test thread, and call.
    pub fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "wgvb-store-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("a writable temporary directory");
        Scratch(path)
    }

    /// A path inside the directory. The file need not exist.
    pub fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Every byte of a file, for asserting that a rejected open wrote nothing.
pub fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("the file exists")
}
