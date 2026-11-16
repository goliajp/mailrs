//! The kevy store's side of accounts, for the shared routes. An account is
//! one JSON blob per address.

use mailrs_core_api::method::admin::{AccountWire, AccountWithHashWire};
use mailrs_core_routes::store::{AccountPatch, AccountStore, NewAccount, StoreError, StoreResult};
use serde_json::{Map, Value};

use crate::FastcoreState;

/// What the delete keeps beside the purge mark and a restore puts back: the
/// account blob, with the password hash and settings in it, and the
/// permissions blob.
#[derive(serde::Deserialize)]
struct Record {
    account: String,
    perms: Option<String>,
}

fn parsed(address: &str, blob: &str) -> StoreResult<AccountWithHashWire> {
    serde_json::from_str(blob).map_err(|e| StoreError(format!("account {address}: {e}")))
}

impl AccountStore for FastcoreState {
    fn net_configured(&self) -> bool {
        self.net_url.is_some()
    }

    async fn exists(&self, address: &str) -> StoreResult<bool> {
        Ok(self.mailbox.get_account_blob(address)?.is_some())
    }

    async fn account(&self, address: &str) -> StoreResult<Option<AccountWithHashWire>> {
        match self.mailbox.get_account_blob(address)? {
            Some(blob) => parsed(address, &blob).map(Some),
            None => Ok(None),
        }
    }

    async fn accounts(&self) -> StoreResult<Vec<AccountWire>> {
        let mut items = Vec::new();
        for address in self.mailbox.list_account_addresses()? {
            let Some(blob) = self.mailbox.get_account_blob(&address)? else {
                continue;
            };
            // one damaged record leaves the rest of the list readable
            match parsed(&address, &blob) {
                Ok(a) => items.push(a.public),
                Err(e) => tracing::warn!(error = %e, "account skipped"),
            }
        }
        Ok(items)
    }

    async fn insert_account(&self, new: &NewAccount<'_>) -> StoreResult<()> {
        let blob = serde_json::json!({
            "address": new.address,
            "domain": new.domain,
            "display_name": new.display_name,
            "active": true,
            "created_at": new.created_at,
            "quota_bytes": new.quota_bytes,
            "recovery_email": null,
            "password_hash": new.password_hash,
        });
        self.mailbox
            .upsert_account(new.address, &blob.to_string())?;
        // the domain dropdowns read the domain index
        if !new.domain.is_empty()
            && let Err(e) = self.mailbox.upsert_domain(new.domain, new.created_at)
        {
            tracing::warn!(error = %e, domain = %new.domain, "domain not indexed for a new account");
        }
        let perms = serde_json::json!({
            "address": new.address,
            "permissions": Vec::<String>::new(),
            "groups": Vec::<Value>::new(),
            "is_super": false,
            "send_as": Vec::<String>::new(),
        });
        if let Err(e) = self
            .mailbox
            .upsert_permissions(new.address, &perms.to_string())
        {
            tracing::warn!(error = %e, address = %new.address, "empty permission set not written");
        }
        Ok(())
    }

    fn maildir_root(&self) -> &std::path::Path {
        std::path::Path::new(&self.maildir_root)
    }

    /// The account blob, with the password hash and settings in it, and the
    /// permissions blob: what the delete drops and a restore puts back.
    async fn record_for_restore(&self, address: &str) -> StoreResult<Option<String>> {
        let account = self.mailbox.get_account_blob(address)?;
        let perms = self.mailbox.get_permissions_blob(address)?;
        Ok(Some(
            serde_json::json!({ "account": account, "perms": perms }).to_string(),
        ))
    }

    async fn drop_account(&self, address: &str, _now: i64) -> StoreResult<()> {
        self.mailbox.delete_account(address)?;
        Ok(())
    }

    async fn patch_account(&self, address: &str, patch: AccountPatch<'_>) -> StoreResult<bool> {
        let Some(blob) = self.mailbox.get_account_blob(address)? else {
            return Ok(false);
        };
        let mut val: Value = serde_json::from_str(&blob)
            .map_err(|e| StoreError(format!("account {address}: {e}")))?;
        let Some(obj) = val.as_object_mut() else {
            return Err(StoreError(format!("account {address}: not an object")));
        };
        apply(obj, patch);
        self.mailbox.upsert_account(address, &val.to_string())?;
        Ok(true)
    }

    async fn bring_back(&self, address: &str, kept: Option<String>) -> StoreResult<bool> {
        let Some(record) = kept
            .as_deref()
            .and_then(|k| serde_json::from_str::<Record>(k).ok())
        else {
            return Ok(false);
        };
        self.mailbox.upsert_account(address, &record.account)?;
        if let Some(perms) = &record.perms {
            self.mailbox.upsert_permissions(address, perms)?;
        }
        Ok(true)
    }
}

fn apply(obj: &mut Map<String, Value>, patch: AccountPatch<'_>) {
    match patch {
        AccountPatch::DisplayName(name) => {
            obj.insert("display_name".into(), name.into());
        }
        AccountPatch::Quota(bytes) => {
            obj.insert("quota_bytes".into(), bytes.into());
        }
        AccountPatch::RecoveryEmail(email) => {
            obj.insert("recovery_email".into(), email.into());
        }
        AccountPatch::PasswordHash(hash) => {
            obj.insert("password_hash".into(), hash.into());
            obj.remove("password_cleared_at");
            obj.remove("password_cleared_by");
        }
        AccountPatch::ClearPassword { by, at } => {
            obj.insert("password_hash".into(), String::new().into());
            obj.insert("password_cleared_at".into(), at.into());
            obj.insert("password_cleared_by".into(), by.into());
        }
    }
}
