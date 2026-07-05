//! Alias resolution — one-level hash lookup in the embedded kevy.
//!
//! Layout:
//! - `mailrs:alias:<address>` string, value = target address
//! - `mailrs:aliases:index`  set  of every alias key we've written
//!
//! Follows a chain up to 4 hops so `a → b → c → d` works while cycles
//! (`a → b → a`) still terminate. Read-only in the hot path — callers
//! that need to mutate use [`upsert_alias`] / [`delete_alias`].

use std::io;

use crate::KevyMailboxStore;
use crate::keys;

const MAX_HOPS: usize = 4;

impl KevyMailboxStore {
    /// Resolve `address` through the alias chain. Returns the terminal
    /// address (`Ok(Some(_))`) or `Ok(None)` when no alias is set.
    /// Cycles are broken by [`MAX_HOPS`]; the last non-loop address is
    /// returned in that case.
    pub fn resolve_alias(&self, address: &str) -> io::Result<Option<String>> {
        let mut current = address.to_string();
        let mut hops = 0;
        let mut hit_any = false;
        while hops < MAX_HOPS {
            let key = keys::alias(&current);
            let Some(raw) = self.store().get(key.as_bytes())? else {
                return Ok(if hit_any { Some(current) } else { None });
            };
            let Ok(next) = std::str::from_utf8(&raw) else {
                return Ok(if hit_any { Some(current) } else { None });
            };
            if next.is_empty() || next == current {
                return Ok(if hit_any { Some(current) } else { None });
            }
            hit_any = true;
            current = next.to_string();
            hops += 1;
        }
        Ok(Some(current))
    }

    /// Point `alias` at `target` (both are full email addresses).
    /// Idempotent — a repeat call with the same target is a no-op.
    pub fn upsert_alias(&self, alias: &str, target: &str) -> io::Result<()> {
        let key = keys::alias(alias);
        self.store().set(key.as_bytes(), target.as_bytes())?;
        self.store()
            .sadd(keys::ALIAS_INDEX.as_bytes(), &[alias.as_bytes()])?;
        Ok(())
    }

    /// Drop an alias entry entirely.
    pub fn delete_alias(&self, alias: &str) -> io::Result<()> {
        let key = keys::alias(alias);
        self.store().del(&[key.as_bytes()])?;
        self.store()
            .srem(keys::ALIAS_INDEX.as_bytes(), &[alias.as_bytes()])?;
        Ok(())
    }

    /// Enumerate every alias for admin listing.
    pub fn list_aliases(&self) -> io::Result<Vec<(String, String)>> {
        let members = self.store().smembers(keys::ALIAS_INDEX.as_bytes())?;
        let mut out = Vec::with_capacity(members.len());
        for m in members {
            let Ok(a) = String::from_utf8(m) else {
                continue;
            };
            let key = keys::alias(&a);
            let Some(raw) = self.store().get(key.as_bytes())? else {
                continue;
            };
            let Ok(target) = String::from_utf8(raw) else {
                continue;
            };
            out.push((a, target));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kevy_embedded::{Config, Store};
    use std::sync::Arc;

    fn store() -> KevyMailboxStore {
        KevyMailboxStore::new(Arc::new(Store::open(Config::default()).unwrap()))
    }

    #[test]
    fn resolve_returns_none_when_unset() {
        let s = store();
        assert!(s.resolve_alias("nobody@example.com").unwrap().is_none());
    }

    #[test]
    fn one_hop_resolve() {
        let s = store();
        s.upsert_alias("contact@example.com", "alice@example.com")
            .unwrap();
        assert_eq!(
            s.resolve_alias("contact@example.com").unwrap().as_deref(),
            Some("alice@example.com")
        );
    }

    #[test]
    fn multi_hop_chain() {
        let s = store();
        s.upsert_alias("a@x", "b@x").unwrap();
        s.upsert_alias("b@x", "c@x").unwrap();
        s.upsert_alias("c@x", "d@x").unwrap();
        assert_eq!(s.resolve_alias("a@x").unwrap().as_deref(), Some("d@x"));
    }

    #[test]
    fn cycle_terminates() {
        let s = store();
        s.upsert_alias("a@x", "b@x").unwrap();
        s.upsert_alias("b@x", "a@x").unwrap();
        // MAX_HOPS caps the walk; we return whatever we settled on.
        let r = s.resolve_alias("a@x").unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn self_alias_stops_immediately() {
        let s = store();
        s.upsert_alias("a@x", "a@x").unwrap();
        assert!(s.resolve_alias("a@x").unwrap().is_none());
    }

    // Regression: case-normalization alias (`Lihao@x -> lihao@x`) must
    // resolve as a legitimate one-hop mapping, not be mistaken for a
    // self-loop. The pre-fix code compared `next` and `current` with
    // `eq_ignore_ascii_case`, which treated the case-differing pair as
    // a cycle and returned None even though a valid target existed.
    #[test]
    fn case_normalization_alias_resolves() {
        let s = store();
        s.upsert_alias("Lihao@x", "lihao@x").unwrap();
        assert_eq!(
            s.resolve_alias("Lihao@x").unwrap().as_deref(),
            Some("lihao@x")
        );
        s.upsert_alias("INFO@golia.jp", "lihao@golia.jp").unwrap();
        assert_eq!(
            s.resolve_alias("INFO@golia.jp").unwrap().as_deref(),
            Some("lihao@golia.jp")
        );
    }

    #[test]
    fn delete_and_list() {
        let s = store();
        s.upsert_alias("c@x", "a@x").unwrap();
        s.upsert_alias("d@x", "a@x").unwrap();
        let mut listed = s.list_aliases().unwrap();
        listed.sort();
        assert_eq!(
            listed,
            vec![
                ("c@x".to_string(), "a@x".to_string()),
                ("d@x".to_string(), "a@x".to_string()),
            ]
        );
        s.delete_alias("c@x").unwrap();
        assert!(s.resolve_alias("c@x").unwrap().is_none());
        assert_eq!(s.list_aliases().unwrap().len(), 1);
    }
}
