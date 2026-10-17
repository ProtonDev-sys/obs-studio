//! Shared support for integration tests.
#![allow(dead_code)]

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

const ISOLATED_TEST: &str = "OBS_UTIL_ISOLATED_TEST";

pub fn is_child(test_name: &str) -> bool {
    std::env::var(ISOLATED_TEST).is_ok_and(|name| name == test_name)
}

/// Re-execute one named test, keeping global counters and cwd changes away
/// from every other test in its category binary. The parent environment is
/// never modified, so parallel callers can each launch their own child.
pub fn in_child(test_name: &str, body: impl FnOnce()) {
    let completed = format!("obs-util isolated test completed: {test_name}");
    if is_child(test_name) {
        body();
        // --nocapture exposes this marker only after the real body returns.
        // Start a new line because libtest has already printed `test ...`.
        println!("\n{completed}");
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", test_name, "--test-threads=1", "--nocapture"])
        .env(ISOLATED_TEST, test_name)
        .output()
        .expect("spawn isolated test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "isolated test {test_name} failed ({})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    // A misspelled --exact filter and an early exit(0) both report success
    // without checking anything. Require proof that the body finished too.
    assert!(
        stdout.lines().any(|line| line == completed),
        "isolated test {test_name} did not complete its body\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// A fresh directory under the temp dir, named `{prefix}-{pid}-{seq}`.
/// It is removed recursively on drop, including when a test or proptest
/// case panics.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Deref for ScratchDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
