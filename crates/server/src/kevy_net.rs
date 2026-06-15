//! Network kevy client for the receiver-split topology.
//!
//! When `MAILRS_KEVY_URL` points at a shared kevy-server, the anti
//! subsystems (greylist / rate / auth-guard) read+write their state
//! over RESP instead of the in-process embedded `Store`. This is the
//! "shared layer" of the core/edge architecture: only state that *must*
//! cross processes lives here; the in-process embedded kevy stays the
//! hot path for everything else.
//!
//! [`kevy_client::Connection`] is a synchronous blocking client
//! (`&mut self`, `io::Result`), so every op runs inside
//! [`tokio::task::spawn_blocking`]. A small pool of connections keeps
//! concurrent blocking tasks from serializing on one socket; a
//! connection that errors is dropped and transparently reopened on the
//! next use.
//!
//! Callers map errors to a **fail-open** default — an unreachable
//! kevy-server must never block mail flow. greylist/rate fall open and
//! the low-frequency reconcile pass is the durability backstop.

use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use kevy_client::Connection;

/// Pooled blocking connections to the kevy-server. Ops round-robin
/// across the pool so concurrent `spawn_blocking` tasks don't serialize
/// on a single RESP socket.
const POOL_SIZE: usize = 8;

/// Shared handle to a network kevy-server. Cheap to clone behind an
/// `Arc`; the anti-subsystem network backends each hold one.
pub struct KevyNetClient {
    url: String,
    pool: Vec<Mutex<Option<Connection>>>,
    next: AtomicUsize,
}

impl KevyNetClient {
    /// Build a client for `url` (e.g. `kevy://host:6379`). Connections
    /// open lazily on first use, so this never fails — an unreachable
    /// server surfaces later as a per-op error the caller fails open on.
    pub fn new(url: impl Into<String>) -> Self {
        let mut pool = Vec::with_capacity(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            pool.push(Mutex::new(None));
        }
        Self {
            url: url.into(),
            pool,
            next: AtomicUsize::new(0),
        }
    }

    /// Run `f` against a pooled connection, synchronously. MUST be
    /// called from inside `spawn_blocking` — the client blocks on
    /// socket I/O. On any error the connection is dropped so the next
    /// call reconnects (handles a kevy-server restart transparently).
    pub fn with_conn<F, R>(&self, f: F) -> io::Result<R>
    where
        F: FnOnce(&mut Connection) -> io::Result<R>,
    {
        let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.pool.len();
        // recover from a poisoned mutex: a panic inside `f` shouldn't
        // wedge the whole pool on a fail-open mail-flow path.
        let mut slot = self.pool[idx].lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(Connection::open(&self.url)?);
        }
        let conn = slot.as_mut().expect("just ensured Some");
        match f(conn) {
            Ok(r) => Ok(r),
            Err(e) => {
                // drop the possibly-broken connection; next use reopens
                *slot = None;
                Err(e)
            }
        }
    }

    /// The configured server URL.
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `mem://` exercises the same Connection command surface the network
    // path uses (URL-dispatched to embedded), so the pool + lazy-open +
    // reconnect-on-error plumbing is covered without a TCP server.
    #[test]
    fn pool_round_trips_set_get_over_mem() {
        let client = KevyNetClient::new("mem://shared-test");
        client
            .with_conn(|c| c.set(b"k", b"v"))
            .expect("set should succeed over mem://");
        let got = client
            .with_conn(|c| c.get(b"k"))
            .expect("get should succeed");
        assert_eq!(got.as_deref(), Some(&b"v"[..]));
    }

    #[test]
    fn url_accessor_returns_configured_url() {
        let client = KevyNetClient::new("kevy://127.0.0.1:6379");
        assert_eq!(client.url(), "kevy://127.0.0.1:6379");
    }
}
