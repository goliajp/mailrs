//! Receiver-facing storage-usage seam for quota enforcement.
//!
//! The SMTP receiving path checks a recipient's current stored size against
//! their quota before accepting a message. That usage query is abstracted
//! behind [`QuotaStore`] so the receiver doesn't bind the spg-backed
//! [`PgMailboxStore`] — in-process today, network-backed in the
//! receiver-split topology (P6). The quota *limit* comes from
//! [`crate::account_store::AccountStore::quota`]; this trait supplies the
//! *usage* it is compared against.

use mailrs_mailbox::PgMailboxStore;

/// The storage-usage lookup the receiving path performs for quota checks.
#[async_trait::async_trait]
pub trait QuotaStore: Send + Sync {
    /// Current total stored bytes for `user` across all their mailboxes.
    /// Infallible by contract — a backend error reports `0` (fail-open: a
    /// transient usage-query failure must not reject otherwise-valid mail).
    async fn user_storage_usage(&self, user: &str) -> u64;
}

#[async_trait::async_trait]
impl QuotaStore for PgMailboxStore {
    async fn user_storage_usage(&self, user: &str) -> u64 {
        PgMailboxStore::user_storage_usage(self, user).await
    }
}
