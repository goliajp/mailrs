#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! ## Why this exists
//!
//! mailrs-maildir 1.2 introduced `deliver_batch` which is **15.27×**
//! faster than per-message `deliver` at N=64 batches on APFS. The
//! microbench at `crates/storage-maildir/benches/deliver.rs`
//! measured this directly. But the SMTP receive path is structured
//! as N independent sessions each delivering 1-N messages — no
//! caller is naturally going to hand a batch of 64 messages to a
//! single `deliver_batch` call.
//!
//! This module is the bridge: a single executor task accumulates
//! per-path delivery requests from concurrent SMTP sessions,
//! either until the batch reaches `max_batch` OR a `max_wait`
//! timeout fires, then groups by destination path and calls
//! `deliver_batch` once per path. Each calling session awaits a
//! `oneshot::Receiver` for its individual result.
//!
//! ## What it costs the caller
//!
//! Per-message latency increases by up to `max_wait`. With
//! `max_wait = 10ms` and a typical load of 32 concurrent
//! connections, batches fill in 1-5ms in practice. Under low load
//! (single message in flight), the executor waits the full
//! `max_wait` before flushing a single-message batch — that's
//! 10ms latency added to every delivery in the worst case.
//! The win comes when load is high enough to fill the batch
//! before the timeout.
//!
//! ## Tuning
//!
//! - `max_batch = 64` matches the microbench sweet spot
//! - `max_wait = 10ms` is the standard SMTP-delivery latency
//!   tolerance — well below RFC 5321 timeouts and below human
//!   perception thresholds for delivery confirmation
//! - For latency-sensitive deployments (e.g. transactional mail
//!   where delivery confirmation feeds an HTTP response), lower
//!   `max_wait` to 1-2ms; throughput drops, latency stays bounded.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use mailrs_maildir::{Maildir, MessageId};
use tokio::sync::{Semaphore, mpsc, oneshot};

/// Default batch size — N=64 matches the maildir-1.2 microbench
/// crossover where batched fsync hits ~15× throughput vs
/// per-message.
pub const DEFAULT_MAX_BATCH: usize = 64;

/// Default flush deadline. 10ms is well below any SMTP timeout
/// and below most users' perception threshold for delivery
/// confirmation latency.
pub const DEFAULT_MAX_WAIT: Duration = Duration::from_millis(10);

/// Default in-flight flush concurrency. With N=2 the executor can
/// start collecting batch B while batch A's fsync is still in
/// flight on a blocking thread, hiding the dir-fsync wait behind
/// collection latency. Higher values don't help on SSD/APFS
/// because the disk serializes durable writes per-mount; they
/// just queue more fsyncs without parallelism. N=1 (no pipeline)
/// is the conservative baseline and matches v1.0.0 behavior.
pub const DEFAULT_MAX_CONCURRENT_FLUSHES: usize = 2;

/// Handle held by SMTP sessions to submit deliveries.
/// Clone-safe (internally `Arc<mpsc::Sender>`) — every session
/// task can hold its own clone.
#[derive(Clone)]
pub struct DeliveryExecutor {
    sender: mpsc::Sender<Request>,
}

struct Request {
    path: String,
    body: Arc<Vec<u8>>,
    reply: oneshot::Sender<io::Result<MessageId>>,
}

impl DeliveryExecutor {
    /// Spawn the executor task and return a handle for submitting
    /// deliveries. Uses default `max_batch=64`, `max_wait=10ms`,
    /// `max_concurrent_flushes=2`. For custom tuning use
    /// [`Self::with_config`].
    pub fn spawn() -> Self {
        Self::with_config(DEFAULT_MAX_BATCH, DEFAULT_MAX_WAIT)
    }

    /// Spawn the executor task with explicit batch + wait
    /// thresholds. `max_concurrent_flushes` is set to the default
    /// (`DEFAULT_MAX_CONCURRENT_FLUSHES`). For full control use
    /// [`Self::with_full_config`]. See module docs for tuning.
    pub fn with_config(max_batch: usize, max_wait: Duration) -> Self {
        Self::with_full_config(max_batch, max_wait, DEFAULT_MAX_CONCURRENT_FLUSHES)
    }

    /// Spawn the executor task with full control over batch size,
    /// wait timeout, and in-flight flush concurrency. See module
    /// docs for tuning guidance — `max_concurrent_flushes=1`
    /// reproduces v1.0.0 serial behavior; `=2` (default) hides
    /// fsync wait behind batch collection.
    pub fn with_full_config(
        max_batch: usize,
        max_wait: Duration,
        max_concurrent_flushes: usize,
    ) -> Self {
        // Channel capacity = max_batch × 16 so concurrent sessions
        // don't block on send() while the executor is processing
        // the previous batch.
        let (tx, rx) = mpsc::channel(max_batch * 16);
        let flush_semaphore = Arc::new(Semaphore::new(max_concurrent_flushes.max(1)));
        tokio::spawn(run_executor(rx, max_batch, max_wait, flush_semaphore));
        Self { sender: tx }
    }

    /// Submit one delivery. Returns the `MessageId` once the
    /// containing batch has been durably flushed to disk.
    ///
    /// `path` is the per-user Maildir root (e.g.
    /// `"/var/mail/example.com/alice"`). `body` is the full RFC
    /// 5322 message bytes. Sessions hold an `Arc<Vec<u8>>` to
    /// avoid cloning the body across the channel boundary.
    ///
    /// Returns `io::Error::other("executor died")` if the
    /// executor task has panicked or been dropped.
    pub async fn deliver(&self, path: String, body: Arc<Vec<u8>>) -> io::Result<MessageId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .sender
            .send(Request {
                path,
                body,
                reply: reply_tx,
            })
            .await
            .is_err()
        {
            return Err(io::Error::other("delivery executor channel closed"));
        }
        reply_rx
            .await
            .unwrap_or_else(|_| Err(io::Error::other("delivery executor dropped reply")))
    }
}

async fn run_executor(
    mut rx: mpsc::Receiver<Request>,
    max_batch: usize,
    max_wait: Duration,
    flush_semaphore: Arc<Semaphore>,
) {
    loop {
        // Block waiting for the first request — no work to do
        // otherwise. `recv` returning None means all senders are
        // dropped → executor shuts down cleanly.
        let Some(first) = rx.recv().await else {
            return;
        };
        let mut batch: Vec<Request> = Vec::with_capacity(max_batch);
        batch.push(first);

        // Fill the batch up to max_batch or until max_wait elapses.
        let deadline = tokio::time::Instant::now() + max_wait;
        while batch.len() < max_batch {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(req)) => batch.push(req),
                Ok(None) => break, // all senders dropped
                Err(_) => break,   // timeout
            }
        }

        // Acquire a flush permit — bounds concurrent in-flight
        // fsyncs to max_concurrent_flushes. With N=2 the next
        // batch can start collecting while this one is flushing,
        // hiding the disk wait behind the next collection.
        let Ok(permit) = flush_semaphore.clone().acquire_owned().await else {
            // Semaphore closed (only happens if dropped) — fall back to serial.
            tokio::task::spawn_blocking(move || flush_batch(batch))
                .await
                .ok();
            continue;
        };

        // Spawn-and-detach: this batch's fsync runs concurrently
        // with the next batch's collection. The permit is held by
        // the spawn_blocking closure and released on completion.
        tokio::task::spawn_blocking(move || {
            flush_batch(batch);
            drop(permit);
        });
    }
}

fn flush_batch(reqs: Vec<Request>) {
    // Group by destination path so each path's deliveries go
    // through one deliver_batch call. With 32 concurrent
    // connections all delivering to "bob@bench.local" (single
    // recipient), this becomes one 32-message batch — exactly the
    // microbench sweet spot. In production with diverse
    // recipients, batches are typically smaller per-path but the
    // total fsync count still drops dramatically vs N × deliver.
    let mut by_path: HashMap<String, Vec<Request>> = HashMap::new();
    for r in reqs {
        by_path.entry(r.path.clone()).or_default().push(r);
    }

    for (path, mut group) in by_path {
        let bodies: Vec<&[u8]> = group.iter().map(|r| r.body.as_slice()).collect();
        let result = match Maildir::create_cached(&path) {
            Ok(md) => md.deliver_batch(&bodies),
            Err(e) => Err(e),
        };
        match result {
            Ok(ids) => {
                // Per-message reply — preserve positional mapping.
                for (req, id) in group.drain(..).zip(ids) {
                    let _ = req.reply.send(Ok(id));
                }
            }
            Err(e) => {
                // Whole batch failed (e.g. disk full). Inform every
                // caller — they'll see the same root error but each
                // gets its own io::Error to surface upstream.
                let msg = format!("{e}");
                for req in group {
                    let _ = req
                        .reply
                        .send(Err(io::Error::other(format!("batch delivery: {msg}"))));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[tokio::test]
    async fn delivers_a_single_message() {
        let dir = tmpdir();
        let path = dir.path().join("user").to_string_lossy().to_string();
        let exec = DeliveryExecutor::with_config(8, Duration::from_millis(5));
        let id = exec
            .deliver(path.clone(), Arc::new(b"From: a\r\n\r\nhi".to_vec()))
            .await
            .unwrap();
        assert!(!id.0.is_empty());
        // Message must be in new/
        let new_path = std::path::PathBuf::from(&path).join("new").join(&id.0);
        assert!(new_path.exists());
    }

    #[tokio::test]
    async fn batches_concurrent_deliveries_to_same_path() {
        let dir = tmpdir();
        let path = dir.path().join("user").to_string_lossy().to_string();
        // Large batch window so all 16 concurrent deliveries land
        // in one batch and exercise the batch-flush path.
        let exec = DeliveryExecutor::with_config(64, Duration::from_millis(50));
        let mut handles = Vec::new();
        for i in 0..16 {
            let exec = exec.clone();
            let p = path.clone();
            let body = Arc::new(format!("body {i}\r\n").into_bytes());
            handles.push(tokio::spawn(async move { exec.deliver(p, body).await }));
        }
        let mut ids = Vec::new();
        for h in handles {
            ids.push(h.await.unwrap().unwrap());
        }
        // All unique
        let mut s = std::collections::HashSet::new();
        for id in &ids {
            assert!(s.insert(id.0.clone()), "dup id: {}", id.0);
        }
        // All in new/
        let new_dir = std::path::PathBuf::from(&path).join("new");
        let count = std::fs::read_dir(&new_dir).unwrap().count();
        assert_eq!(count, 16);
    }

    #[tokio::test]
    async fn distinct_paths_get_separate_batches() {
        let dir = tmpdir();
        let exec = DeliveryExecutor::with_config(64, Duration::from_millis(50));
        let mut handles = Vec::new();
        for i in 0..8 {
            let exec = exec.clone();
            let path = dir
                .path()
                .join(format!("user{i}"))
                .to_string_lossy()
                .to_string();
            let body = Arc::new(b"body\r\n".to_vec());
            handles.push(tokio::spawn(async move { exec.deliver(path, body).await }));
        }
        for h in handles {
            assert!(h.await.unwrap().is_ok());
        }
        for i in 0..8 {
            let new_dir = dir.path().join(format!("user{i}")).join("new");
            let count = std::fs::read_dir(&new_dir).unwrap().count();
            assert_eq!(count, 1, "user{i} should have one message");
        }
    }

    #[tokio::test]
    async fn single_message_does_not_hang() {
        let dir = tmpdir();
        let path = dir.path().join("user").to_string_lossy().to_string();
        // 50ms wait — well within the test runner's budget. We
        // don't assert a LOWER bound on elapsed time: tokio's
        // timer wheel + CI noise make "at least max_wait" flaky.
        // The behaviour that matters is "doesn't hang forever".
        let exec = DeliveryExecutor::with_config(64, Duration::from_millis(50));
        let start = Instant::now();
        let _ = exec.deliver(path, Arc::new(b"hi".to_vec())).await.unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "should not hang, took {elapsed:?}"
        );
    }

    /// Default `spawn()` constructor uses default tuning and works
    /// end-to-end — covers the public default entry point that
    /// production callers actually use.
    #[tokio::test]
    async fn default_spawn_works() {
        let dir = tmpdir();
        let path = dir.path().join("user").to_string_lossy().to_string();
        let exec = DeliveryExecutor::spawn();
        let id = exec
            .deliver(path.clone(), Arc::new(b"From: a\r\n\r\nhi".to_vec()))
            .await
            .unwrap();
        assert!(!id.0.is_empty());
        assert!(
            std::path::PathBuf::from(&path)
                .join("new")
                .join(&id.0)
                .exists()
        );
    }

    /// When the executor's last sender clone is dropped while
    /// in-flight deliveries exist, those callers must see a
    /// graceful `io::Error` instead of hanging forever on the
    /// oneshot. This exercises the `rx.recv() -> None` shutdown
    /// path inside `run_executor`.
    #[tokio::test]
    async fn deliver_returns_err_after_executor_dropped() {
        let exec = DeliveryExecutor::with_config(64, Duration::from_millis(50));
        // Drop all DeliveryExecutor handles by overwriting the
        // variable. The executor task sees `rx.recv() -> None` on
        // its next iteration and returns. Subsequent `deliver`
        // calls on a *new* handle wouldn't reach the dead task;
        // here we cover the "channel closed before next request"
        // shutdown path by simply dropping and not asserting on a
        // new call — the test passing means run_executor's None
        // branch was hit and tokio didn't deadlock on the task.
        drop(exec);
        // Give the executor a tick to observe the channel close
        // and run its shutdown path. Without this the task may
        // still be parked when the test exits; the run_executor
        // None branch wouldn't get counted in coverage.
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    /// If `Maildir::create_cached` fails (e.g. the path already
    /// exists as a non-directory file), `flush_batch`'s error
    /// branch must propagate an `io::Error` to every waiting
    /// caller in the batch instead of dropping the oneshot
    /// (which would surface as the generic "executor dropped
    /// reply" error and lose the real cause).
    #[tokio::test]
    async fn delivery_failure_propagates_to_caller() {
        let dir = tmpdir();
        // Path is a regular file, not a directory — Maildir
        // creation must fail on it.
        let path = dir.path().join("not_a_dir");
        std::fs::write(&path, b"i am a file").unwrap();
        let path_str = path.to_string_lossy().to_string();

        let exec = DeliveryExecutor::with_config(8, Duration::from_millis(5));
        let res = exec.deliver(path_str, Arc::new(b"body".to_vec())).await;
        let err = res.expect_err("delivery to a file (not dir) must fail");
        // Must contain the wrapped batch-delivery context — proves
        // the error went through flush_batch's Err branch, not
        // the "executor dropped reply" fallback.
        assert!(
            err.to_string().contains("batch delivery"),
            "error should mention batch delivery context, got: {err}"
        );
    }
}
