//! What the shared maintenance routes need of a store beyond accounts:
//! the addresses, the local domains and what each account may send as.

use std::future::Future;

use mailrs_core_api::method::admin::EffectivePermissionsResponse;

use super::{AccountStore, StoreResult};

pub trait MaintenanceStore: AccountStore {
    /// Every account address, including one whose record does not parse.
    fn account_addresses(&self) -> impl Future<Output = StoreResult<Vec<String>>> + Send;

    /// The domains this server receives mail for.
    fn domain_names(&self) -> impl Future<Output = StoreResult<Vec<String>>> + Send;

    /// The permissions sending checks `user` against; `None` when they
    /// cannot be read, which grants no address.
    fn send_as_permissions(
        &self,
        user: &str,
    ) -> impl Future<Output = Option<EffectivePermissionsResponse>> + Send;
}
