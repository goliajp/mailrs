//! Alias resolution — one-level hash lookup in the embedded kevy.
//!
//! Layout:
//! - `mailrs:alias:<address>` string, value = target address
//! - `mailrs:aliases:index`  set  of every alias key we've written
//!
//! Follows a chain up to 4 hops so `a → b → c → d` works while cycles
//! (`a → b → a`) still terminate. Read-only in the hot path — callers
//! that need to mutate use [`upsert_alias`] / [`delete_alias`].
//!
//! Exposes both the historical inherent-method surface (used by existing
//! call sites) and the backend-agnostic [`mailrs_alias_store::AliasStore`]
//! trait so fastcore / pg-core state can hold `Arc<dyn AliasStore>`
//! without caring which store is behind it.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use mailrs_alias_store::AliasStore;

use crate::KevyMailboxStore;
use crate::keys;

const MAX_HOPS: usize = 4;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl KevyMailboxStore {
    /// Resolve `address` through the alias chain. Returns the terminal
    /// address (`Ok(Some(_))`) or `Ok(None)` when no alias is set.
    /// Cycles are broken by [`MAX_HOPS`]; the last non-loop address is
    /// returned in that case.
    ///
    /// v2.6.2 §P6 legacy drop: reads the v2 hash `target` field first,
    /// falls back to the legacy string only when v2 is absent — that
    /// covers pre-Phase-9 rows that were removed from the boot backfill
    /// path (`resolve_alias` runs before backfill on the SMTP hot path
    /// during ingress).
    pub fn resolve_alias(&self, address: &str) -> io::Result<Option<String>> {
        let mut current = address.to_string();
        let mut hops = 0;
        let mut hit_any = false;
        while hops < MAX_HOPS {
            let key_v2 = keys::alias_v2(&current);
            let raw = match self.store().hget(key_v2.as_bytes(), b"target")? {
                Some(bytes) => Some(bytes),
                None => {
                    let key = keys::alias(&current);
                    self.store().get(key.as_bytes())?
                }
            };
            let Some(raw) = raw else {
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
    ///
    /// v2.6.2 §P6 legacy drop: writes only the v2 hash. `resolve_alias`
    /// still reads the legacy string key for pre-Phase-9 rows that
    /// haven't been touched (backfill promotes them at boot); post-
    /// Phase-11 aliases exist only on the v2 hash.
    pub fn upsert_alias(&self, alias: &str, target: &str) -> io::Result<()> {
        let key_v2 = keys::alias_v2(alias);
        let domain = alias.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let created_at = now_secs().to_string();
        self.store().atomic(|ctx| {
            ctx.hset(
                key_v2.as_bytes(),
                &[
                    (b"target".as_slice(), target.as_bytes()),
                    (b"domain".as_slice(), domain.as_bytes()),
                    (b"created_at".as_slice(), created_at.as_bytes()),
                    (b"active".as_slice(), b"1".as_slice()),
                ],
            )?;
            Ok(())
        })
    }

    /// Drop an alias entry entirely.
    ///
    /// v2.6.2 §P6 legacy drop: DELs both the legacy string key and the
    /// v2 hash so old aliases still get cleaned up. `srem` on the legacy
    /// set kept because backfill reads it — dropping the srem would
    /// leave a stale entry the backfill re-promotes on next boot.
    pub fn delete_alias(&self, alias: &str) -> io::Result<()> {
        let key = keys::alias(alias);
        let key_v2 = keys::alias_v2(alias);
        self.store().atomic(|ctx| {
            ctx.del(&[key.as_bytes(), key_v2.as_bytes()]);
            ctx.srem(keys::ALIAS_INDEX.as_bytes(), &[alias.as_bytes()])?;
            Ok(())
        })
    }

    /// Enumerate every alias for admin listing.
    ///
    /// v2.6.1c §P6-C: switched to `Store::idx_query` on
    /// `aliases_by_domain`. Sync in-process so N sequential HGETs
    /// on embedded are cheap (~sub-µs each); the win is server-side
    /// domain-sorted order + parity with the network side that
    /// Phase 10 already cut over.
    pub fn list_aliases(&self) -> io::Result<Vec<(String, String)>> {
        use kevy_embedded::IndexValue;
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let (rows, next) = self.store().idx_query(
                keys::IDX_ALIASES_BY_DOMAIN,
                &IndexValue::Str(Vec::new()),
                &IndexValue::Str(vec![0xff, 0xff]),
                cursor.as_ref(),
                10_000,
            )?;
            for (key, _domain_val) in rows {
                let Some(addr_bytes) = key.strip_prefix(keys::ALIAS_V2_PREFIX) else {
                    continue;
                };
                let Ok(addr) = std::str::from_utf8(addr_bytes) else {
                    continue;
                };
                let Some(target_bytes) = self.store().hget(&key, b"target")? else {
                    continue;
                };
                let Ok(target) = String::from_utf8(target_bytes) else {
                    continue;
                };
                out.push((addr.to_string(), target));
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(out)
    }

    /// v2.6.1c §P6-C embedded backfill: promote every pre-Phase-9
    /// alias from the legacy string+set layout into the v2 hash so
    /// `list_aliases` (now `idx_query`-based) covers rows written
    /// before dual-write started. Idempotent — skips rows that
    /// already have a v2 `target`.
    pub fn backfill_admin_v2(&self) -> io::Result<AdminBackfillStats> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
            .to_string();

        let mut stats = AdminBackfillStats::default();

        // ── aliases ────────────────────────────────────────────
        let members = self.store().smembers(keys::ALIAS_INDEX.as_bytes())?;
        for m in members {
            let Ok(addr) = String::from_utf8(m) else {
                continue;
            };
            let key_v2 = keys::alias_v2(&addr);
            if self.store().hget(key_v2.as_bytes(), b"target")?.is_some() {
                continue;
            }
            let key = keys::alias(&addr);
            let Some(target) = self.store().get(key.as_bytes())? else {
                continue;
            };
            let domain = addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
            self.store().hset(
                key_v2.as_bytes(),
                &[
                    (b"target".as_slice(), target.as_slice()),
                    (b"domain".as_slice(), domain.as_bytes()),
                    (b"created_at".as_slice(), now.as_bytes()),
                    (b"active".as_slice(), b"1".as_slice()),
                ],
            )?;
            stats.aliases += 1;
        }

        // ── domains ────────────────────────────────────────────
        let members = self.store().smembers(keys::DOMAIN_INDEX.as_bytes())?;
        for m in members {
            let Ok(name) = String::from_utf8(m) else {
                continue;
            };
            let key_v2 = keys::domain_v2(&name);
            if self
                .store()
                .hget(key_v2.as_bytes(), b"created_at")?
                .is_some()
            {
                continue;
            }
            let key = keys::domain(&name);
            let Some(created_bytes) = self.store().get(key.as_bytes())? else {
                continue;
            };
            self.store().hset(
                key_v2.as_bytes(),
                &[(b"created_at".as_slice(), created_bytes.as_slice())],
            )?;
            stats.domains += 1;
        }

        // ── accounts ───────────────────────────────────────────
        let members = self.store().smembers(keys::ACCOUNT_INDEX.as_bytes())?;
        for m in members {
            let Ok(addr) = String::from_utf8(m) else {
                continue;
            };
            let key = keys::account(&addr);
            if self.store().hget(key.as_bytes(), b"domain")?.is_some() {
                continue;
            }
            let Some(blob_bytes) = self.store().hget(key.as_bytes(), b"blob")? else {
                continue;
            };
            let blob = String::from_utf8_lossy(&blob_bytes);
            let (active, created_at) = crate::account::derive_account_fields(&blob);
            let domain = addr.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
            self.store().hset(
                key.as_bytes(),
                &[
                    (b"domain".as_slice(), domain.as_bytes()),
                    (b"active".as_slice(), active),
                    (b"created_at".as_slice(), created_at.as_bytes()),
                ],
            )?;
            stats.accounts += 1;
        }

        Ok(stats)
    }
}

/// v2.6.1c §P6-C backfill counters — reported at fastcore boot for
/// forensic visibility.
#[derive(Default, Debug, Clone, Copy)]
pub struct AdminBackfillStats {
    pub aliases: usize,
    pub domains: usize,
    pub accounts: usize,
}

/// Bridge the embedded-kevy alias implementation to the shared
/// [`AliasStore`] contract. Every method delegates to the inherent one
/// so the historical call sites and the trait-based ones are two names
/// for the same code — no behaviour drift possible.
impl AliasStore for KevyMailboxStore {
    fn resolve(&self, address: &str) -> io::Result<Option<String>> {
        self.resolve_alias(address)
    }

    fn upsert(&self, source: &str, target: &str) -> io::Result<()> {
        self.upsert_alias(source, target)
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        let key = keys::alias(source);
        let key_v2 = keys::alias_v2(source);
        self.store().atomic(|ctx| {
            let removed = ctx.del(&[key.as_bytes(), key_v2.as_bytes()]);
            ctx.srem(keys::ALIAS_INDEX.as_bytes(), &[source.as_bytes()])?;
            Ok(removed > 0)
        })
    }

    fn list(&self) -> io::Result<Vec<(String, String)>> {
        self.list_aliases()
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

    #[test]
    fn trait_contract_case_and_delete() {
        // Exercise the KevyMailboxStore via the AliasStore trait so the
        // fastcore state's `Arc<dyn AliasStore>` path is covered by the
        // same guarantees the inherent-method tests give.
        let s = store();
        let t: &dyn mailrs_alias_store::AliasStore = &s;
        t.upsert("Lihao@x", "lihao@x").unwrap();
        assert_eq!(t.resolve("Lihao@x").unwrap().as_deref(), Some("lihao@x"));
        assert!(t.delete("Lihao@x").unwrap());
        assert!(!t.delete("Lihao@x").unwrap()); // idempotent second delete
        assert!(t.resolve("Lihao@x").unwrap().is_none());
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
        // v2.6.2 §P6: list_aliases walks the aliases_by_domain range
        // index — declare it before any upsert so the writes populate.
        s.ensure_admin_indexes();
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
