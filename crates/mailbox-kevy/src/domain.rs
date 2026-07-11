//! Domain rows on kevy — the switchable mail-store's domain list, mirror
//! of the pg-core `domains` table. Layout:
//! - `mailrs:domain:<name>` string, value = created_at epoch seconds
//! - `mailrs:domains:index` set of every domain name
//!
//! Kept minimal (name + created_at) to match `DomainWire`.

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

impl KevyMailboxStore {
    /// Insert a domain (idempotent). `created_at` is only set on first
    /// write so re-adding keeps the original timestamp.
    pub fn upsert_domain(&self, name: &str, created_at: i64) -> io::Result<()> {
        let key = keys::domain(name);
        let key_v2 = keys::domain_v2(name);
        let created_str = created_at.to_string();
        self.store().atomic(|ctx| {
            if ctx.get(key.as_bytes())?.is_none() {
                ctx.set(key.as_bytes(), created_str.as_bytes());
            }
            ctx.sadd(keys::DOMAIN_INDEX.as_bytes(), &[name.as_bytes()])?;
            // v2.6.0 §P6 dual-write: hash + range-indexed created_at.
            ctx.hset(
                key_v2.as_bytes(),
                &[(b"created_at".as_slice(), created_str.as_bytes())],
            )?;
            Ok(())
        })
    }

    /// Remove a domain. Returns whether it existed.
    pub fn delete_domain(&self, name: &str) -> io::Result<bool> {
        let key = keys::domain(name);
        let key_v2 = keys::domain_v2(name);
        self.store().atomic(|ctx| {
            let existed = ctx.get(key.as_bytes())?.is_some();
            ctx.del(&[key.as_bytes(), key_v2.as_bytes()]);
            ctx.srem(keys::DOMAIN_INDEX.as_bytes(), &[name.as_bytes()])?;
            Ok(existed)
        })
    }

    /// List every domain as `(name, created_at)`, sorted by name.
    pub fn list_domains(&self) -> io::Result<Vec<(String, i64)>> {
        let members = self.store().smembers(keys::DOMAIN_INDEX.as_bytes())?;
        let mut out = Vec::new();
        for m in members {
            let Ok(name) = String::from_utf8(m) else {
                continue;
            };
            let created_at = self
                .store()
                .get(keys::domain(&name).as_bytes())?
                .and_then(|v| String::from_utf8(v).ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            out.push((name, created_at));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}
