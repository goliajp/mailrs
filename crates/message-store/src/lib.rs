//! Async storage-backend abstraction for delivered mail.
//!
//! [`MessageStore`] is the seam the receiver/core split (and a future
//! object-store backend) share: the receiver writes via `deliver_batch`,
//! the core reads + maintains via `fetch` / `list_unprocessed` /
//! `mark_processed` / `delete`. [`MaildirStore`] is the default,
//! maildir-backed implementation; the planned S3 backend (P7) is the
//! second.
//!
//! It is **path-keyed**: every call carries the per-recipient maildir root
//! (for an object backend, the equivalent prefix), so one store serves all
//! users without per-user state.

use std::io;

use async_trait::async_trait;
use mailrs_maildir::Maildir;

pub use mailrs_maildir::{Entry, Flag, MessageId};

/// A pluggable backend for delivered-message storage. See the module docs.
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Deliver `bodies` to the store under `path`, returning the assigned
    /// ids in input order.
    async fn deliver_batch(&self, path: &str, bodies: &[&[u8]]) -> io::Result<Vec<MessageId>>;

    /// Read the raw bytes of message `id` under `path`; `Ok(None)` if it is
    /// not present.
    async fn fetch(&self, path: &str, id: &MessageId) -> io::Result<Option<Vec<u8>>>;

    /// List the unprocessed (newly-delivered) messages under `path`.
    async fn list_unprocessed(&self, path: &str) -> io::Result<Vec<Entry>>;

    /// Mark `id` under `path` processed, recording `flags`.
    async fn mark_processed(&self, path: &str, id: &MessageId, flags: &[Flag]) -> io::Result<()>;

    /// Delete `id` under `path`.
    async fn delete(&self, path: &str, id: &MessageId) -> io::Result<()>;
}

/// Maildir-backed [`MessageStore`]. Stateless — each call opens the maildir
/// for the given `path`. The blocking filesystem work runs on
/// `spawn_blocking` so it never stalls the async runtime.
#[derive(Debug, Clone, Copy, Default)]
pub struct MaildirStore;

#[async_trait]
impl MessageStore for MaildirStore {
    async fn deliver_batch(&self, path: &str, bodies: &[&[u8]]) -> io::Result<Vec<MessageId>> {
        let path = path.to_string();
        // own the bodies so the blocking closure is 'static
        let owned: Vec<Vec<u8>> = bodies.iter().map(|b| b.to_vec()).collect();
        blocking(move || {
            let md = Maildir::create_cached(&path)?;
            let refs: Vec<&[u8]> = owned.iter().map(|b| b.as_slice()).collect();
            md.deliver_batch(&refs)
        })
        .await
    }

    async fn fetch(&self, path: &str, id: &MessageId) -> io::Result<Option<Vec<u8>>> {
        let path = path.to_string();
        let id = id.clone();
        blocking(move || Maildir::open(&path).fetch(&id)).await
    }

    async fn list_unprocessed(&self, path: &str) -> io::Result<Vec<Entry>> {
        let path = path.to_string();
        blocking(move || Maildir::open(&path).scan_new()).await
    }

    async fn mark_processed(&self, path: &str, id: &MessageId, flags: &[Flag]) -> io::Result<()> {
        let path = path.to_string();
        let id = id.clone();
        let flags = flags.to_vec();
        blocking(move || Maildir::open(&path).mark_processed(&id, &flags)).await
    }

    async fn delete(&self, path: &str, id: &MessageId) -> io::Result<()> {
        let path = path.to_string();
        let id = id.clone();
        blocking(move || Maildir::open(&path).delete(&id)).await
    }
}

/// Run a blocking maildir op on the blocking pool, flattening the
/// `JoinError` into the `io::Result`.
async fn blocking<F, T>(f: F) -> io::Result<T>
where
    F: FnOnce() -> io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r,
        Err(e) => Err(io::Error::other(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn maildir_store_round_trips_via_trait() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("u").to_string_lossy().into_owned();
        let store = MaildirStore;

        // deliver -> two unprocessed
        let ids = store
            .deliver_batch(&path, &[&b"one"[..], &b"two"[..]])
            .await
            .unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(store.list_unprocessed(&path).await.unwrap().len(), 2);

        // fetch round-trips the bytes
        assert_eq!(
            store.fetch(&path, &ids[0]).await.unwrap().as_deref(),
            Some(&b"one"[..])
        );

        // mark_processed moves one out of unprocessed
        store
            .mark_processed(&path, &ids[0], &[Flag::Seen])
            .await
            .unwrap();
        assert_eq!(store.list_unprocessed(&path).await.unwrap().len(), 1);
        // still fetchable from cur/
        assert!(store.fetch(&path, &ids[0]).await.unwrap().is_some());

        // delete removes it
        store.delete(&path, &ids[1]).await.unwrap();
        assert!(store.fetch(&path, &ids[1]).await.unwrap().is_none());
    }
}
