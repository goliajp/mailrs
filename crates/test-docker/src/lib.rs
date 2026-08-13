//! One container comes up at a time, across the whole workspace.
//!
//! Six fixtures in four crates start their own container — four pgvector, one
//! kevy, one mailpit — and `cargo test --workspace` runs every test binary in
//! parallel. Under that spike the wait-for-ready times out and tests go red
//! while passing one at a time.
//!
//! That is worse than a missing test. A red test that is red because of its
//! neighbours teaches the reader that red does not mean broken, and the next
//! genuine failure gets waved through. This repo has the lesson written down
//! already; what it did not have was one place to fix it.
//!
//! Only the STARTUP is serialised. Once a container is up, running against it
//! in parallel is fine, and holding the lock for a test's duration would turn a
//! parallel suite into a serial one.
//!
//! Cross-PROCESS, because cargo gives each test binary its own process — an
//! in-process `Mutex` would serialise one crate's tests and leave the other
//! three racing, which is exactly the half-fix this crate exists to replace.
//! Hence a lock file, and hence the stale-breaking below.
//!
//! ```no_run
//! # async fn f() {
//! let _guard = mailrs_test_docker::startup_lock().await;
//! // ... start the container, wait for ready ...
//! // guard drops here, and the next test's container may start
//! # }
//! ```

use std::path::PathBuf;
use std::time::Duration;

/// Abandoned after this long. Generous relative to a container start; the cost
/// of being wrong is one extra concurrent startup, and the cost of no timeout
/// at all is a crashed run wedging every later one.
const STALE_AFTER: Duration = Duration::from_secs(120);

/// Held while one container comes up. Released on drop, including on panic.
pub struct StartupLock(PathBuf);

impl Drop for StartupLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Wait for the lock, break it if its holder is long gone, take it.
///
/// `create_new` is the atomic part — exactly one process wins the race to
/// create the file. Everything else here is liveness.
pub async fn startup_lock() -> StartupLock {
    let path = std::env::temp_dir().join("mailrs-test-container-startup.lock");
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return StartupLock(path),
            Err(_) => {
                let abandoned = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| t.elapsed().unwrap_or_default() > STALE_AFTER)
                    // Unreadable metadata means the file went away under us, or
                    // was never really there: try to take it rather than wait
                    // forever on something that may not exist.
                    .unwrap_or(true);
                if abandoned {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_lock_is_exclusive_and_releases_on_drop() {
        let first = startup_lock().await;
        let path = first.0.clone();
        assert!(path.exists(), "holding the lock means the file is there");

        // A second acquire must not succeed while the first is held. Bounded so
        // a broken lock fails the test rather than hanging the suite — the
        // mistake made once already in this repo's tie-boundary test.
        let blocked = tokio::time::timeout(Duration::from_millis(600), startup_lock()).await;
        assert!(blocked.is_err(), "the lock let two holders in at once");

        drop(first);
        assert!(!path.exists(), "dropping the lock removes the file");
        // And it is takeable again.
        let _second = tokio::time::timeout(Duration::from_secs(2), startup_lock())
            .await
            .expect("the lock must be available once released");
    }
}
