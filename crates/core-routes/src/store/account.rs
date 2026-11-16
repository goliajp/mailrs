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

    /// Whether `address` has an account, without reading it whole.
    fn exists(&self, address: &str) -> impl Future<Output = StoreResult<bool>> + Send;

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

    /// Where the account maildirs are, which a delete moves aside.
    fn maildir_root(&self) -> &std::path::Path;

    /// What to keep beside the purge mark for a restore to put back, for a
    /// store that drops the account itself; `None` for one that keeps the
    /// deleted account in place.
    fn record_for_restore(
        &self,
        address: &str,
    ) -> impl Future<Output = StoreResult<Option<String>>> + Send;

    /// The account stops being one; harmless when there is none.
    fn drop_account(&self, address: &str, now: i64)
    -> impl Future<Output = StoreResult<()>> + Send;

    /// Purge what the deleted account at `address` owns now, rather than
    /// after the retention period. Failures are logged: the retention task
    /// retries.
    fn purge_now(
        self: std::sync::Arc<Self>,
        address: String,
        now: i64,
    ) -> impl Future<Output = ()> + Send;

    /// Put back the account a delete dropped, from `kept` (what
    /// [`Self::record_for_restore`] gave, if anything). `false` when there
    /// is nothing to put back.
    fn bring_back(
        &self,
        address: &str,
        kept: Option<String>,
    ) -> impl Future<Output = StoreResult<bool>> + Send;
}
