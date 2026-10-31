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
        let res = super::account::clear_password(self, address, by.to_string()).await;
        match res.status() {
            axum::http::StatusCode::NO_CONTENT => Ok(true),
            axum::http::StatusCode::NOT_FOUND => Ok(false),
            s => Err(io::Error::other(format!("clearing answered {s}"))),
        }
    }

    fn net_url(&self) -> Option<String> {
        self.net_url.clone()
    }
}
