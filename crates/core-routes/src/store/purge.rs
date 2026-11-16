//! A store's part of purging a deleted account or a deleted group: what it
//! holds for them outside the shared store and the maildir.

use std::future::Future;
use std::sync::Arc;

use mailrs_core_api::method::admin::PurgeCount;

use super::{AccountStore, StoreResult};

pub trait PurgeStore: AccountStore {
    /// What this store holds for `address`, by kind; kinds with nothing
    /// may be left out.
    fn count_mail(
        self: &Arc<Self>,
        address: &str,
    ) -> impl Future<Output = StoreResult<Vec<PurgeCount>>> + Send;

    /// Remove what this store holds for `address`. A conversation another
    /// owner still holds keeps its shared records.
    fn remove_mail(self: &Arc<Self>, address: &str)
    -> impl Future<Output = StoreResult<()>> + Send;

    /// Records in this store that name `address` but that no purge step
    /// knows about: reported, never removed.
    fn mail_unregistered(
        self: &Arc<Self>,
        address: &str,
    ) -> impl Future<Output = StoreResult<Vec<PurgeCount>>> + Send;

    /// The reply claims of the group `gid`'s conversations, counted, and
    /// removed unless `dry_run`.
    fn purge_group_replies(
        self: &Arc<Self>,
        gid: i64,
        dry_run: bool,
    ) -> impl Future<Output = StoreResult<PurgeCount>> + Send;
}
