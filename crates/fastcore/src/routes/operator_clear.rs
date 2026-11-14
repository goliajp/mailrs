//! fastcore's store for the operator's password clearing; the procedure is
//! `mailrs_core_sidestate::families::password_clear`.

use std::io;

use mailrs_core_sidestate::families::password_clear::PasswordStore;

use crate::FastcoreState;

impl PasswordStore for FastcoreState {
    async fn cleared_at(&self, address: &str) -> io::Result<Option<Option<i64>>> {
        let Some(blob) = self.mailbox.get_account_blob(address)? else {
            return Ok(None);
        };
        let v: serde_json::Value = serde_json::from_str(&blob).map_err(io::Error::other)?;
        Ok(Some(v["password_cleared_at"].as_i64()))
    }

    async fn clear_password(&self, address: &str, by: &str) -> io::Result<bool> {
        let at = chrono::Utc::now().timestamp();
        let patch = mailrs_core_routes::store::AccountPatch::ClearPassword { by, at };
        mailrs_core_routes::store::AccountStore::patch_account(self, address, patch)
            .await
            .map_err(io::Error::other)
    }

    fn net_url(&self) -> Option<String> {
        self.net_url.clone()
    }
}
