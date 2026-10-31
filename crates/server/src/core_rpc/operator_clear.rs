//! The SQL core's store for the operator's password clearing; the procedure
//! is `mailrs_core_sidestate::families::password_clear`.

use std::io;

use mailrs_core_sidestate::families::password_clear::PasswordStore;

use crate::core_rpc::CoreRpcState;

impl PasswordStore for CoreRpcState {
    async fn cleared_at(&self, address: &str) -> io::Result<Option<Option<i64>>> {
        let found = self
            .domain
            .get_account_with_hash(address)
            .await
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        Ok(found.map(|(account, _)| account.password_cleared_at))
    }

    async fn clear_password(&self, address: &str, by: &str) -> io::Result<bool> {
        self.domain
            .clear_password(address, by, chrono::Utc::now().timestamp())
            .await
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }

    fn net_url(&self) -> Option<String> {
        self.net_url.clone()
    }
}
