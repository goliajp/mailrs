//! In-process adapter for the receiver's [`QuotaStore`] port.
//!
//! `PgMailboxStore` is a foreign type (mailrs-mailbox stone) and
//! `QuotaStore` is a foreign trait (mailrs-receiver), so the orphan rule
//! forbids a direct impl here — wrap it in a local newtype.

use std::sync::Arc;

use mailrs_mailbox::PgMailboxStore;
use mailrs_receiver::QuotaStore;

/// Local adapter so the spg-backed mailbox store can serve the receiver's
/// quota-usage port.
pub struct MailboxQuotaStore(pub Arc<PgMailboxStore>);

#[async_trait::async_trait]
impl QuotaStore for MailboxQuotaStore {
    async fn user_storage_usage(&self, user: &str) -> u64 {
        self.0.user_storage_usage(user).await
    }
}
