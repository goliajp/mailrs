//! In-process adapter for the receiver's [`AccountStore`] port: the
//! spg-backed [`DomainStore`] implementation. The port (trait) lives in
//! `mailrs-receiver`; this is the core-side adapter.

use mailrs_receiver::{AccountStore, AccountStoreError};

use std::sync::Arc;

use crate::domain_store::DomainStore;

/// The domain store as the receiver's account port: submission auth.
pub(crate) struct DomainAccounts(pub Arc<DomainStore>);

impl std::ops::Deref for DomainAccounts {
    type Target = DomainStore;
    fn deref(&self) -> &DomainStore {
        &self.0
    }
}

#[async_trait::async_trait]
impl AccountStore for DomainAccounts {
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError> {
        Ok(self
            .get_account_with_hash(address)
            .await
            .map_err(|e| AccountStoreError::Backend(e.to_string()))?
            .map(|(_account, hash)| hash))
    }
}
