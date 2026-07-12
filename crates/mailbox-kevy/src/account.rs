//! Account backend — persist `AccountWithHashWire` as a JSON blob in
//! a kevy hash. Phase 8 lets fastcore serve login + effective_permissions
//! straight from kevy so webapi never touches spg for auth.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::KevyMailboxStore;
use crate::keys;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Extract `active` from an `AccountWithHashWire` JSON blob. Defaults
/// to `"1"` (active) when the field is absent — matches the pg-side
/// invariant that a row without an explicit deactivation is live.
fn extract_active(blob_json: &str) -> &'static [u8] {
    match serde_json::from_str::<serde_json::Value>(blob_json) {
        Ok(v) => match v.get("active").and_then(|x| x.as_bool()) {
            Some(false) => b"0",
            _ => b"1",
        },
        Err(_) => b"1",
    }
}

/// Extract `created_at` epoch seconds from the blob, falling back to
/// "now" when the field is absent (only on the first upsert; subsequent
/// upserts preserve whatever the caller passes in the blob).
fn extract_created_at(blob_json: &str) -> i64 {
    serde_json::from_str::<serde_json::Value>(blob_json)
        .ok()
        .and_then(|v| v.get("created_at").and_then(|x| x.as_i64()))
        .unwrap_or_else(now_secs)
}

/// v2.6.1c §P6-C backfill helper: crate-visible so `alias.rs` (which
/// owns the aggregate `backfill_admin_v2` walker) can derive the same
/// `(active, created_at_string)` pair without duplicating the parse.
pub(crate) fn derive_account_fields(blob_json: &str) -> (&'static [u8], String) {
    (
        extract_active(blob_json),
        extract_created_at(blob_json).to_string(),
    )
}

impl KevyMailboxStore {
    /// UPSERT an account. `blob_json` is the JSON-serialized
    /// `AccountWithHashWire`. Adds the address to `ACCOUNT_INDEX` so
    /// admin/list_accounts + pg-dump can walk it.
    ///
    /// v2.6.0 §P6 dual-write: additionally stamps `domain`, `active`,
    /// `created_at` derived from the address / blob so
    /// `accounts_by_domain` + `accounts_by_active` range indexes stay
    /// current. Fields are derived and safe to overwrite on every
    /// upsert (they mirror the row's identity).
    pub fn upsert_account(&self, address: &str, blob_json: &str) -> io::Result<()> {
        let key = keys::account(address);
        let domain = address.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let active = extract_active(blob_json);
        let created_at = extract_created_at(blob_json).to_string();
        self.store().atomic(|ctx| {
            ctx.hset(
                key.as_bytes(),
                &[
                    (b"blob".as_slice(), blob_json.as_bytes()),
                    (b"domain".as_slice(), domain.as_bytes()),
                    (b"active".as_slice(), active),
                    (b"created_at".as_slice(), created_at.as_bytes()),
                ],
            )?;
            ctx.sadd(keys::ACCOUNT_INDEX.as_bytes(), &[address.as_bytes()])?;
            Ok(())
        })
    }

    /// Load the raw JSON blob for `address`. Returns `Ok(None)` when
    /// the account doesn't exist.
    pub fn get_account_blob(&self, address: &str) -> io::Result<Option<String>> {
        let key = keys::account(address);
        let raw = self.store().hget(key.as_bytes(), b"blob")?;
        match raw {
            Some(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("account blob: {e}"))
            })?)),
            None => Ok(None),
        }
    }

    /// List every registered account address.
    ///
    /// v2.6.1c §P6-C: switched to `Store::idx_query` on
    /// `accounts_by_active`. Range covers all values so the result
    /// set matches the pre-cutover admin-list contract.
    pub fn list_account_addresses(&self) -> io::Result<Vec<String>> {
        use kevy_embedded::IndexValue;
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (rows, next) = self.store().idx_query(
                keys::IDX_ACCOUNTS_BY_ACTIVE,
                &IndexValue::Str(Vec::new()),
                &IndexValue::Str(vec![0xff, 0xff]),
                cursor.as_ref(),
                10_000,
            )?;
            for (key, _) in rows {
                let Some(addr_bytes) = key.strip_prefix(keys::ACCOUNT_PREFIX) else {
                    continue;
                };
                let Ok(addr) = std::str::from_utf8(addr_bytes) else {
                    continue;
                };
                out.push(addr.to_string());
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(out)
    }

    /// Upsert the effective_permissions JSON blob for `address`.
    pub fn upsert_permissions(&self, address: &str, blob_json: &str) -> io::Result<()> {
        let key = keys::account_permissions(address);
        self.store().set(key.as_bytes(), blob_json.as_bytes())?;
        Ok(())
    }

    /// Load the effective_permissions JSON blob for `address`, if any.
    pub fn get_permissions_blob(&self, address: &str) -> io::Result<Option<String>> {
        let key = keys::account_permissions(address);
        let raw = self.store().get(key.as_bytes())?;
        match raw {
            Some(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("perms blob: {e}"))
            })?)),
            None => Ok(None),
        }
    }

    /// Remove account + perms blob + drop from the account index. Does
    /// NOT touch the user's threads / mailboxes / maildir — the caller
    /// is responsible for that when a hard delete is desired.
    pub fn delete_account(&self, address: &str) -> io::Result<()> {
        let acct = keys::account(address);
        let perms = keys::account_permissions(address);
        self.store().atomic(|ctx| {
            ctx.del(&[acct.as_bytes(), perms.as_bytes()]);
            ctx.srem(keys::ACCOUNT_INDEX.as_bytes(), &[address.as_bytes()])?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> Arc<Store> {
        Arc::new(Store::open(Config::default()).unwrap())
    }

    #[test]
    fn upsert_get_roundtrip() {
        let s = KevyMailboxStore::new(store());
        s.upsert_account("a@x", r#"{"address":"a@x"}"#).unwrap();
        assert_eq!(
            s.get_account_blob("a@x").unwrap().as_deref(),
            Some(r#"{"address":"a@x"}"#)
        );
        assert_eq!(s.get_account_blob("missing@x").unwrap(), None);
    }

    #[test]
    fn list_addresses() {
        let s = KevyMailboxStore::new(store());
        s.upsert_account("a@x", "{}").unwrap();
        s.upsert_account("b@x", "{}").unwrap();
        let mut addrs = s.list_account_addresses().unwrap();
        addrs.sort();
        assert_eq!(addrs, vec!["a@x".to_string(), "b@x".to_string()]);
    }

    #[test]
    fn permissions_blob() {
        let s = KevyMailboxStore::new(store());
        s.upsert_permissions("a@x", r#"{"is_super":true}"#).unwrap();
        assert_eq!(
            s.get_permissions_blob("a@x").unwrap().as_deref(),
            Some(r#"{"is_super":true}"#)
        );
    }
}
