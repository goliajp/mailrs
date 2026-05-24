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
use tokio::sync::{mpsc, oneshot};

/// Default batch size — N=64 matches the maildir-1.2 microbench
/// crossover where batched fsync hits ~15× throughput vs
/// per-message.
pub const DEFAULT_MAX_BATCH: usize = 64;

/// Default flush deadline. 10ms is well below any SMTP timeout
/// and below most users' perception threshold for delivery
/// confirmation latency.
pub const DEFAULT_MAX_WAIT: Duration = Duration::from_millis(10);

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
    /// deliveries. Uses default `max_batch=64`, `max_wait=10ms`.
    /// For custom tuning use [`Self::with_config`].
    pub fn spawn() -> Self {
        Self::with_config(DEFAULT_MAX_BATCH, DEFAULT_MAX_WAIT)
    }

    /// Spawn the executor task with explicit batch + wait
    /// thresholds. See module docs for tuning guidance.
    pub fn with_config(max_batch: usize, max_wait: Duration) -> Self {
        // Channel capacity = max_batch × 16 so concurrent sessions
        // don't block on send() while the executor is processing
        // the previous batch.
        let (tx, rx) = mpsc::channel(max_batch * 16);
        tokio::spawn(run_executor(rx, max_batch, max_wait));
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

async fn run_executor(mut rx: mpsc::Receiver<Request>, max_batch: usize, max_wait: Duration) {
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

        // Flush the batch on a blocking thread so we don't tie up
        // the tokio runtime during the fsync wait.
        tokio::task::spawn_blocking(move || flush_batch(batch))
            .await
            .ok(); // task panic shouldn't kill the executor; just drop the batch
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
        let _ = exec
            .deliver(path, Arc::new(b"hi".to_vec()))
            .await
            .unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "should not hang, took {elapsed:?}"
        );
    }
}
