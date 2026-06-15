//! Receiver-facing storage-usage port for quota enforcement.

/// The storage-usage lookup the receiving path performs for quota checks.
/// The quota *limit* comes from [`crate::AccountStore::quota`]; this trait
/// supplies the *usage* it is compared against.
#[async_trait::async_trait]
pub trait QuotaStore: Send + Sync {
    /// Current total stored bytes for `user` across all their mailboxes.
    /// Infallible by contract — a backend error reports `0` (fail-open: a
    /// transient usage-query failure must not reject otherwise-valid mail).
    async fn user_storage_usage(&self, user: &str) -> u64;
}
