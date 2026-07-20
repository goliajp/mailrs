//! Alias resolution — v2 hash-field lookup in the embedded kevy.
//!
//! Layout: `mailrs:alias:v2:<address>` hash `{target, domain,
//! created_at, active}`; range-indexed by `aliases_by_domain` /
//! `aliases_by_target`.
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
    pub fn resolve_alias(&self, address: &str) -> io::Result<Option<String>> {
        let mut current = address.to_string();
        let mut hops = 0;
        let mut hit_any = false;
        while hops < MAX_HOPS {
            let key_v2 = keys::alias_v2(&current);
            let mut target = self.store().hget(key_v2.as_bytes(), b"target")?;
            // Domain catch-all. An entry keyed `@example.com` answers for
            // every local-part in that domain that has no explicit alias
            // and no mailbox of its own. Without it, mail to an address
            // nobody thought to enumerate has nowhere to go and sits in
            // the spool indefinitely — three real messages sat there for
            // up to 11 days before anyone noticed (2026-07-20).
            //
            // Only consulted on the first hop: a catch-all is a policy
            // for inbound addresses, not a link in an alias chain, and
            // letting it fire mid-chain would silently redirect a
            // deliberate alias whose target was merely misspelled.
            if target.is_none()
                && hops == 0
                && let Some((_, domain)) = current.rsplit_once('@')
            {
                let key = keys::alias_v2(&format!("@{domain}"));
                target = self.store().hget(key.as_bytes(), b"target")?;
            }
            let Some(raw) = target else {
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
    pub fn delete_alias(&self, alias: &str) -> io::Result<()> {
        let key_v2 = keys::alias_v2(alias);
        self.store().atomic(|ctx| {
            ctx.del(&[key_v2.as_bytes()]);
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
        let key_v2 = keys::alias_v2(source);
        self.store().atomic(|ctx| {
            let removed = ctx.del(&[key_v2.as_bytes()]);
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
    fn domain_catch_all_answers_for_unlisted_local_parts() {
        let s = store();
        s.upsert_alias("@example.com", "lihao@example.com").unwrap();
        // nobody enumerated `purchase@` — the catch-all takes it
        assert_eq!(
            s.resolve_alias("purchase@example.com").unwrap().as_deref(),
            Some("lihao@example.com")
        );
    }

    #[test]
    fn explicit_alias_beats_the_catch_all() {
        let s = store();
        s.upsert_alias("@example.com", "lihao@example.com").unwrap();
        s.upsert_alias("sales@example.com", "bob@example.com")
            .unwrap();
        assert_eq!(
            s.resolve_alias("sales@example.com").unwrap().as_deref(),
            Some("bob@example.com")
        );
    }

    #[test]
    fn catch_all_does_not_capture_a_broken_alias_target() {
        // `contact@` deliberately points at `gone@`, which has no
        // mailbox. The catch-all must NOT quietly re-route that second
        // hop to itself — a deliberate alias with a bad target is a
        // configuration error to surface, not to paper over.
        let s = store();
        s.upsert_alias("@example.com", "lihao@example.com").unwrap();
        s.upsert_alias("contact@example.com", "gone@example.com")
            .unwrap();
        assert_eq!(
            s.resolve_alias("contact@example.com").unwrap().as_deref(),
            Some("gone@example.com")
        );
    }

    #[test]
    fn catch_all_is_scoped_to_its_own_domain() {
        let s = store();
        s.upsert_alias("@example.com", "lihao@example.com").unwrap();
        assert!(s.resolve_alias("someone@other.com").unwrap().is_none());
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
