//! This core as the shared maintenance routes see it.

use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_core_routes::store::{MaintenanceStore, StoreError, StoreResult};

use super::CoreRpcState;

impl MaintenanceStore for CoreRpcState {
    async fn account_addresses(&self) -> StoreResult<Vec<String>> {
        let accounts = self
            .domain
            .list_accounts()
            .await
            .map_err(|e| StoreError(e.to_string()))?;
        Ok(accounts.into_iter().map(|a| a.address).collect())
    }

    async fn domain_names(&self) -> StoreResult<Vec<String>> {
        self.domain
            .domain_names()
            .await
            .map_err(|e| StoreError(e.to_string()))
    }

    async fn send_as_permissions(&self, user: &str) -> Option<EffectivePermissionsResponse> {
        super::handlers::admin::effective_permissions_of(
            &self.domain,
            self.net_url.as_deref(),
            user,
        )
        .await
        .ok()
    }
}
