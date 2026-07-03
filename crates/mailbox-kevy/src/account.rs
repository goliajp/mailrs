//! Account backend — persist `AccountWithHashWire` as a JSON blob in
//! a kevy hash. Phase 8 lets fastcore serve login + effective_permissions
//! straight from kevy so webapi never touches spg for auth.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// UPSERT an account. `blob_json` is the JSON-serialized
    /// `AccountWithHashWire`. Adds the address to `ACCOUNT_INDEX` so
    /// admin/list_accounts + pg-dump can walk it.
    pub fn upsert_account(&self, address: &str, blob_json: &str) -> io::Result<()> {
        let key = keys::account(address);
        self.store()
            .hset(key.as_bytes(), &[(b"blob", blob_json.as_bytes())])?;
        self.store()
            .sadd(keys::ACCOUNT_INDEX.as_bytes(), &[address.as_bytes()])?;
        Ok(())
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
    pub fn list_account_addresses(&self) -> io::Result<Vec<String>> {
        let members = self.store().smembers(keys::ACCOUNT_INDEX.as_bytes())?;
        members
            .into_iter()
            .map(|m| {
                String::from_utf8(m)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("addr: {e}")))
            })
            .collect()
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
        self.store().del(&[acct.as_bytes(), perms.as_bytes()])?;
        self.store()
            .srem(keys::ACCOUNT_INDEX.as_bytes(), &[address.as_bytes()])?;
        Ok(())
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
