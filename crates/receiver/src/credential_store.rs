//! Where submission auth finds an account's password hash when the
//! accounts live in another process.
//!
//! In the split topology the accounts are in fastcore's embedded kevy,
//! which only fastcore can open. webapi's login and fastcore's IMAP read
//! the same record; this asks fastcore for it over core RPC, so a
//! password that signs in to the web UI also authenticates on 587 / 465.

use crate::AccountStoreError;

#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync {
    /// The argon2 hash for `address`, `None` when there is no such account.
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError>;

    /// What `address` may do, `None` when there is no such account.
    async fn effective_permissions(
        &self,
        address: &str,
    ) -> Result<
        Option<mailrs_core_api::method::admin::EffectivePermissionsResponse>,
        AccountStoreError,
    >;

    /// Whether an administrator has cleared `address`'s password: `false`
    /// for no such account.
    async fn password_cleared(&self, address: &str) -> Result<bool, AccountStoreError>;
}

/// Reads `GET /v1/admin/accounts/{address}/credentials` from the core.
pub struct CoreCredentialStore {
    client: mailrs_core_api::client::Client,
}

impl CoreCredentialStore {
    pub fn new(base_url: String, secret: String) -> Self {
        Self {
            client: mailrs_core_api::client::Client::new(base_url, secret),
        }
    }
}

#[async_trait::async_trait]
impl CredentialStore for CoreCredentialStore {
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError> {
        match self.client.get_account_with_hash(address).await {
            Ok(account) => Ok(account.password_hash),
            Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => Ok(None),
            Err(e) => Err(AccountStoreError::Backend(e.to_string())),
        }
    }

    async fn effective_permissions(
        &self,
        address: &str,
    ) -> Result<
        Option<mailrs_core_api::method::admin::EffectivePermissionsResponse>,
        AccountStoreError,
    > {
        match self.client.effective_permissions(address).await {
            Ok(p) => Ok(Some(p)),
            Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => Ok(None),
            Err(e) => Err(AccountStoreError::Backend(e.to_string())),
        }
    }

    async fn password_cleared(&self, address: &str) -> Result<bool, AccountStoreError> {
        match self.client.get_account_with_hash(address).await {
            Ok(account) => Ok(account.public.password_cleared_at.is_some()),
            Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => Ok(false),
            Err(e) => Err(AccountStoreError::Backend(e.to_string())),
        }
    }
}
