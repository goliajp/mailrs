//! Accounts: who may sign in, with what, and the settings an operator sets.

use std::future::Future;

use mailrs_core_api::method::admin::{AccountWire, AccountWithHashWire};
use mailrs_core_sidestate::NetKevy;

use super::StoreResult;

/// An account about to be made; the store has none at `address`.
pub struct NewAccount<'a> {
    pub address: &'a str,
    pub domain: &'a str,
    pub display_name: &'a str,
    pub password_hash: &'a str,
    pub created_at: i64,
    pub quota_bytes: i64,
}

/// One change to an existing account.
pub enum AccountPatch<'a> {
    DisplayName(&'a str),
    Quota(i64),
    RecoveryEmail(&'a str),
    /// A password set is no longer a password cleared.
    PasswordHash(&'a str),
    /// No password signs in until a new one is set.
    ClearPassword {
        by: &'a str,
        at: i64,
    },
}

/// The shared store (quota limits, purge marks, audit) is reached through
/// [`NetKevy`]; whether one is configured at all is [`Self::net_configured`],
/// since "none configured" and "configured but unreachable" are answered
/// differently.
pub trait AccountStore: NetKevy {
    fn net_configured(&self) -> bool;

    fn account(
        &self,
        address: &str,
    ) -> impl Future<Output = StoreResult<Option<AccountWithHashWire>>> + Send;

    fn accounts(&self) -> impl Future<Output = StoreResult<Vec<AccountWire>>> + Send;

    /// Make the account, its domain if the store keeps domains, and an empty
    /// permission set if it keeps those beside the account.
    fn insert_account(&self, new: &NewAccount<'_>) -> impl Future<Output = StoreResult<()>> + Send;

    /// `false` when there is no account at `address`.
    fn patch_account(
        &self,
        address: &str,
        patch: AccountPatch<'_>,
    ) -> impl Future<Output = StoreResult<bool>> + Send;
}
