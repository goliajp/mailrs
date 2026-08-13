//! In-process adapter for the receiver's [`AccountStore`] port: the
//! spg-backed [`DomainStore`] implementation. The port (trait) lives in
//! `mailrs-receiver`; this is the core-side adapter.

use mailrs_receiver::{AccountStore, AccountStoreError, ResolvedRecipient};

use crate::domain_store::DomainStore;

#[async_trait::async_trait]
impl AccountStore for DomainStore {
    async fn resolve_recipient(&self, address: &str) -> ResolvedRecipient {
        DomainStore::resolve_recipient(self, address).await
    }

    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError> {
        Ok(self
            .get_account_with_hash(address)
            .await
            .map_err(|e| AccountStoreError::Backend(e.to_string()))?
            .map(|(_account, hash)| hash))
    }

    async fn sieve_script(&self, address: &str) -> Result<Option<String>, AccountStoreError> {
        // Not off `self`: a Sieve script is not SQL data. It lives in the
        // shared network store, which is where every other reader looks —
        // see `crate::sieve_store`.
        crate::sieve_store::get(address).map_err(|e| AccountStoreError::Backend(e.to_string()))
    }

    async fn quota(&self, address: &str) -> Result<Option<i64>, AccountStoreError> {
        self.get_quota(address)
            .await
            .map_err(|e| AccountStoreError::Backend(e.to_string()))
    }

    async fn should_send_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        period_secs: u64,
    ) -> Result<bool, AccountStoreError> {
        DomainStore::should_send_vacation_reply(self, recipient, sender, handle, period_secs)
            .await
            .map_err(|e| AccountStoreError::Backend(e.to_string()))
    }

    async fn record_vacation_reply(
        &self,
        recipient: &str,
        sender: &str,
        handle: &str,
        now: i64,
    ) -> Result<(), AccountStoreError> {
        DomainStore::record_vacation_reply(self, recipient, sender, handle, now)
            .await
            .map_err(|e| AccountStoreError::Backend(e.to_string()))
    }
}
