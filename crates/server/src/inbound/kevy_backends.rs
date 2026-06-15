//! Network-kevy implementations of the anti-subsystem storage traits.
//!
//! These live in the server (cement), not in the stones, so the
//! published stones (`mailrs-shield`, `mailrs-rate-limit`,
//! `mailrs-auth-guard`) stay free of a network-client dependency. Each
//! wraps the blocking [`kevy_client::Connection`] (via
//! [`KevyNetClient`]) in `spawn_blocking` and folds errors into the
//! **fail-open** shape its trait already assumes — an unreachable
//! kevy-server must never block mail flow.
//!
//! Used only in the receiver-split topology (`MAILRS_KEVY_URL` set);
//! otherwise the subsystems run on the in-process embedded store.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use mailrs_shield::greylist::GreylistBackend;

use crate::kevy_net::KevyNetClient;

/// Network [`GreylistBackend`] over a shared kevy-server.
///
/// Fail-open: a read error reads as "not seen" (→ the policy greylists,
/// i.e. delays, rather than letting unverified mail straight through),
/// and writes are best-effort — exactly the shape
/// [`GreylistBackend`] documents.
pub struct KevyServerGreylistBackend {
    client: Arc<KevyNetClient>,
}

impl KevyServerGreylistBackend {
    pub fn new(client: Arc<KevyNetClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl GreylistBackend for KevyServerGreylistBackend {
    async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let client = self.client.clone();
        let key = key.to_vec();
        // JoinError → None, io::Error → None, miss → None: every failure
        // mode collapses to "not seen", the safe greylist default.
        tokio::task::spawn_blocking(move || client.with_conn(|c| c.get(&key)))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
    }

    async fn set_with_ttl(&self, key: &[u8], value: &[u8], ttl: Duration) {
        let client = self.client.clone();
        let key = key.to_vec();
        let value = value.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| c.set_with_ttl(&key, &value, ttl))
        })
        .await;
    }

    async fn expire(&self, key: &[u8], ttl: Duration) {
        let client = self.client.clone();
        let key = key.to_vec();
        let _ = tokio::task::spawn_blocking(move || {
            client.with_conn(|c| c.expire(&key, ttl).map(|_| ()))
        })
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercise the backend over `mem://` (URL-dispatched to embedded),
    // which drives the same Connection command surface the network path
    // uses — proves get/set_with_ttl round-trip through spawn_blocking.
    #[tokio::test]
    async fn greylist_backend_set_then_get_round_trip() {
        let client = Arc::new(KevyNetClient::new("mem://greylist-backend-test"));
        let backend = KevyServerGreylistBackend::new(client);

        assert_eq!(backend.get(b"gl:triplet").await, None);
        backend
            .set_with_ttl(b"gl:triplet", b"1700000000", Duration::from_secs(3600))
            .await;
        assert_eq!(
            backend.get(b"gl:triplet").await.as_deref(),
            Some(&b"1700000000"[..])
        );
    }
}
