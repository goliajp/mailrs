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

/// Abandoned after this long **since the holder last said it was alive**.
///
/// It used to mean "since the file was created", which is a different
/// claim: a container that legitimately took longer than this to report
/// ready had its lock broken by a waiter, which then started a second
/// container beside it — the serialisation defeating itself under
/// exactly the load it exists for. With a heartbeat the two are
/// separable, so this can be short: a dead holder is now detected in
/// seconds rather than minutes, and a slow one is never mistaken for
/// dead however long it takes.
const STALE_AFTER: Duration = Duration::from_secs(30);

/// How often a holder touches the file to say it is still there.
/// Comfortably inside `STALE_AFTER`, so a missed tick is not a
/// breakage.
const HEARTBEAT: Duration = Duration::from_secs(5);

/// Held while one container comes up. Released on drop, including on panic.
pub struct StartupLock(PathBuf, Option<tokio::task::JoinHandle<()>>);

impl Drop for StartupLock {
    fn drop(&mut self) {
        if let Some(h) = self.1.take() {
            h.abort();
        }
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
            Ok(_) => {
                // Say so, periodically, for as long as this is held.
                // Without it the waiter below cannot tell a holder that
                // is taking a long time from one that is gone.
                let beat_path = path.clone();
                let beat = tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(HEARTBEAT).await;
                        // Touch by rewriting the mtime. An error means
                        // the file is gone, and then there is nothing
                        // left to keep alive.
                        if filetime_now(&beat_path).is_err() {
                            return;
                        }
                    }
                });
                return StartupLock(path, Some(beat));
            }
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

/// Set a file's mtime to now, without changing its contents.
fn filetime_now(path: &std::path::Path) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new().write(true).open(path)?;
    // A zero-length write is not enough on every filesystem; setting the
    // length to what it already is updates mtime and touches no bytes.
    let len = f.metadata()?.len();
    f.set_len(len)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A slow holder is not a dead one**, and the file's age cannot
    /// tell them apart on its own.
    ///
    /// `STALE_AFTER` exists so a crashed run does not wedge every later
    /// one. But nothing refreshed the file, so "not modified for 120 s"
    /// meant "created 120 s ago" — and a container that legitimately
    /// takes longer than that to report ready had its lock taken away
    /// by a waiter, which then started a second container beside it.
    /// The serialisation defeated itself under exactly the load it was
    /// written for: on 2026-08-17 three `mailrs-mailbox` tests went red
    /// with `WaitContainer(StartupTimeout)` during a deploy gate, 541 s
    /// for eleven tests, and the same eleven passed in 55 s alone.
    ///
    /// So the holder touches the file while it holds it, and the age
    /// that matters is the age since the last touch.
    #[tokio::test]
    async fn a_holder_that_is_merely_slow_keeps_its_lock() {
        let held = startup_lock().await;
        let path = held.0.clone();

        // Longer than the heartbeat, so a lock that is not refreshed
        // shows its age here.
        tokio::time::sleep(HEARTBEAT * 3).await;

        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default())
            .expect("the lock file is readable while held");
        assert!(
            age < HEARTBEAT * 2,
            "the lock file has not been touched for {age:?}; a waiter would \
             read that as an abandoned lock and start a second container \
             beside a live one"
        );
        drop(held);
    }

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
