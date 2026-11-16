//! The SQL store's side of accounts, for the shared routes.

use mailrs_core_api::method::admin::{AccountWire, AccountWithHashWire};
use mailrs_core_routes::store::{AccountPatch, AccountStore, NewAccount, StoreError, StoreResult};

use super::CoreRpcState;
use crate::domain_store::Account;

fn failed(e: impl std::fmt::Display) -> StoreError {
    StoreError(e.to_string())
}

fn wire(a: Account) -> AccountWire {
    AccountWire {
        address: a.address,
        domain: a.domain,
        display_name: a.display_name,
        active: a.active,
        created_at: a.created_at,
        quota_bytes: a.quota_bytes,
        recovery_email: match a.recovery_email.is_empty() {
            true => None,
            false => Some(a.recovery_email),
        },
        password_cleared_at: a.password_cleared_at,
        password_cleared_by: a.password_cleared_by,
    }
}

impl AccountStore for CoreRpcState {
    fn net_configured(&self) -> bool {
        self.net_url.is_some()
    }

    async fn exists(&self, address: &str) -> StoreResult<bool> {
        super::handlers::admin::account_purge::is_account(self, address)
            .await
            .map_err(|code| StoreError(format!("reading the account answered {code}")))
    }

    async fn account(&self, address: &str) -> StoreResult<Option<AccountWithHashWire>> {
        let found = self
            .domain
            .get_account_with_hash(address)
            .await
            .map_err(failed)?;
        Ok(found.map(|(a, hash)| AccountWithHashWire {
            public: wire(a),
            password_hash: Some(hash),
        }))
    }

    async fn accounts(&self) -> StoreResult<Vec<AccountWire>> {
        let rows = self.domain.list_accounts().await.map_err(failed)?;
        Ok(rows.into_iter().map(wire).collect())
    }

    async fn insert_account(&self, new: &NewAccount<'_>) -> StoreResult<()> {
        // the accounts row references its domain, which a freshly migrated
        // core may not have yet
        if !new.domain.is_empty() {
            sqlx::query("INSERT INTO domains (name) VALUES ($1) ON CONFLICT DO NOTHING")
                .bind(new.domain)
                .execute(&self.pool)
                .await
                .map_err(failed)?;
        }
        self.domain
            .add_account(
                new.address,
                new.domain,
                new.display_name,
                new.password_hash,
                new.created_at,
            )
            .await
            .map_err(failed)
    }

    fn maildir_root(&self) -> &std::path::Path {
        std::path::Path::new(&self.maildir_root)
    }

    /// The deleted account stays in its row, marked, for a restore.
    async fn record_for_restore(&self, _address: &str) -> StoreResult<Option<String>> {
        Ok(None)
    }

    async fn drop_account(&self, address: &str, now: i64) -> StoreResult<()> {
        self.domain
            .remove_account(address, now)
            .await
            .map(|_| ())
            .map_err(failed)
    }

    async fn purge_now(self: std::sync::Arc<Self>, address: String, now: i64) {
        let purged =
            super::handlers::admin::account_purge::run(&self, false, Some(address.clone()), now, 0)
                .await;
        if let Err(code) = purged {
            tracing::error!(%code, %address, "purging at the delete failed; the retention task retries");
        }
    }

    async fn patch_account(&self, address: &str, patch: AccountPatch<'_>) -> StoreResult<bool> {
        let d = &self.domain;
        let done = match patch {
            AccountPatch::DisplayName(name) => d.update_account_display_name(address, name).await,
            AccountPatch::Quota(bytes) => d.set_quota(address, bytes).await,
            AccountPatch::RecoveryEmail(email) => d.update_recovery_email(address, email).await,
            AccountPatch::PasswordHash(hash) => d.set_password_hash(address, hash).await,
            AccountPatch::ClearPassword { by, at } => d.clear_password(address, by, at).await,
        };
        done.map_err(failed)
    }

    /// The row never left; restoring clears its `deleted_at`.
    async fn bring_back(&self, address: &str, _kept: Option<String>) -> StoreResult<bool> {
        self.domain.restore_account(address).await.map_err(failed)
    }
}
