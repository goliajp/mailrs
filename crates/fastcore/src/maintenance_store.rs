//! The kevy store as the shared maintenance routes see it.

use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_core_routes::store::{MaintenanceStore, StoreResult};

use crate::FastcoreState;

impl MaintenanceStore for FastcoreState {
    async fn account_addresses(&self) -> StoreResult<Vec<String>> {
        Ok(self.mailbox.list_account_addresses()?)
    }

    async fn domain_names(&self) -> StoreResult<Vec<String>> {
        Ok(self
            .mailbox
            .list_domains()?
            .into_iter()
            .map(|(name, _)| name)
            .collect())
    }

    /// Never `None`: an unreadable part costs the addresses it gives, as
    /// in sending.
    async fn send_as_permissions(&self, user: &str) -> Option<EffectivePermissionsResponse> {
        Some(crate::routes::send_as_permissions_blocking(self, user))
    }
}
