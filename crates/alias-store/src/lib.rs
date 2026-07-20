//! Backend-agnostic alias table trait.
//!
//! Fastcore uses [`mailrs_mailbox_kevy::KevyMailboxStore`] as an impl backed
//! by embedded kevy; pg-core / monolith use the PG `aliases` table via
//! [`mailrs_server::domain_store`]. Both mount the same `/v1/admin/aliases`
//! contract from `mailrs_core_api::method::admin` so callers don't care.
//!
//! The trait is intentionally minimal: 4 methods, all sync `io::Result`.
//! Async wrappers live at the caller (axum handlers already spawn on a
//! blocking pool for the current KevyMailboxStore paths).
//!
//! See `.claude/rfcs/20260705-alias-sync-across-stacks.md` for the design.

use std::io;

/// Address-to-address lookup + admin surface for one process's alias table.
///
/// Case semantics: implementations MUST NOT normalize case internally.
/// SMTP-layer lowercasing happens at the receiver's RCPT parser (that's
/// where identity is defined); the store just persists what it's told.
/// Cycle / self-alias handling belongs to the impl (kevy uses a hop cap
/// + byte-eq self-loop guard, see [`mailrs_mailbox_kevy::KevyMailboxStore`]).
pub trait AliasStore: Send + Sync {
    /// Resolve `address` through the alias chain. `Ok(Some(_))` = the
    /// terminal address; `Ok(None)` = no alias set (deliver to original,
    /// bounce, whatever the caller decides).
    ///
    /// **Domain catch-all is part of this contract.** An entry stored
    /// under `@example.com` answers for any address in that domain with
    /// no entry of its own, and is consulted on the **first hop only** —
    /// it is inbound policy, not a link in a chain, so it must not
    /// re-route a deliberate alias whose target happens to be wrong.
    /// An explicit entry always wins over the catch-all.
    ///
    /// Verify an implementation with
    /// [`assert_catch_all_contract`] rather than hand-rolling the
    /// cases: this project shipped a catch-all in one impl while the
    /// one production actually runs kept dropping mail (2026-07-20).
    fn resolve(&self, address: &str) -> io::Result<Option<String>>;

    /// Point `source` at `target`. Idempotent — re-issuing with the same
    /// target is a no-op.
    fn upsert(&self, source: &str, target: &str) -> io::Result<()>;

    /// Drop `source` from the table. Returns `true` if a row existed.
    fn delete(&self, source: &str) -> io::Result<bool>;

    /// Enumerate `(source, target)` pairs for admin listing / dumping.
    /// Order is impl-defined; callers should sort if display order matters.
    fn list(&self) -> io::Result<Vec<(String, String)>>;
}

/// In-memory `AliasStore` for tests (and any caller that just needs a
/// throwaway table). Threadsafe via a Mutex — not meant for hot paths.
#[derive(Default)]
pub struct MemoryAliasStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryAliasStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AliasStore for MemoryAliasStore {
    fn resolve(&self, address: &str) -> io::Result<Option<String>> {
        // Match KevyMailboxStore's hop-cap semantics so tests written
        // against the trait behave uniformly regardless of backend.
        const MAX_HOPS: usize = 4;
        let g = self.inner.lock().unwrap();
        let mut current = address.to_string();
        let mut hit = false;
        for hop in 0..MAX_HOPS {
            // First hop may fall back to the domain catch-all; see the
            // trait docs for why later hops must not.
            let direct = g.get(&current);
            let entry = match (direct, hop) {
                (None, 0) => current
                    .rsplit_once('@')
                    .and_then(|(_, domain)| g.get(&format!("@{domain}"))),
                (other, _) => other,
            };
            match entry {
                Some(next) if !next.is_empty() && next != &current => {
                    hit = true;
                    current = next.clone();
                }
                _ => return Ok(if hit { Some(current) } else { None }),
            }
        }
        Ok(Some(current))
    }

    fn upsert(&self, source: &str, target: &str) -> io::Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(source.to_string(), target.to_string());
        Ok(())
    }

    fn delete(&self, source: &str) -> io::Result<bool> {
        Ok(self.inner.lock().unwrap().remove(source).is_some())
    }

    fn list(&self) -> io::Result<Vec<(String, String)>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_impl_roundtrips_and_resolves_chain() {
        let s = MemoryAliasStore::new();
        s.upsert("a@x", "b@x").unwrap();
        s.upsert("b@x", "c@x").unwrap();
        assert_eq!(s.resolve("a@x").unwrap().as_deref(), Some("c@x"));
    }

    #[test]
    fn memory_impl_case_normalization_alias_resolves() {
        // Same regression the kevy backend needed (bug fix 2026-07-05).
        let s = MemoryAliasStore::new();
        s.upsert("Lihao@x", "lihao@x").unwrap();
        assert_eq!(s.resolve("Lihao@x").unwrap().as_deref(), Some("lihao@x"));
    }

    #[test]
    fn memory_impl_self_alias_returns_none() {
        let s = MemoryAliasStore::new();
        s.upsert("a@x", "a@x").unwrap();
        assert!(s.resolve("a@x").unwrap().is_none());
    }

    #[test]
    fn memory_impl_delete_reports_prior_existence() {
        let s = MemoryAliasStore::new();
        assert!(!s.delete("nobody@x").unwrap());
        s.upsert("nobody@x", "somebody@x").unwrap();
        assert!(s.delete("nobody@x").unwrap());
        assert!(s.resolve("nobody@x").unwrap().is_none());
    }
}

/// Assert that `store` honours the domain catch-all contract described
/// on [`AliasStore::resolve`].
///
/// Exists because the contract was implemented in one backend while the
/// one production actually runs went without it, and mail addressed to
/// unenumerated local parts sat undelivered for eleven days. Every
/// implementation should call this from its own test module; a backend
/// that cannot run hermetically (the network one) should call it from
/// its opt-in integration test.
///
/// Uses `catchall-contract.test` as its domain so it cannot collide
/// with fixtures the caller set up.
pub fn assert_catch_all_contract(store: &dyn AliasStore) {
    const DOM: &str = "catchall-contract.test";
    let addr = |local: &str| format!("{local}@{DOM}");

    store.upsert(&format!("@{DOM}"), &addr("inbox")).unwrap();

    // 1. answers for a local part nobody enumerated
    assert_eq!(
        store.resolve(&addr("never-listed")).unwrap(),
        Some(addr("inbox")),
        "catch-all must answer for unlisted local parts"
    );

    // 2. an explicit entry outranks it
    store.upsert(&addr("sales"), &addr("bob")).unwrap();
    assert_eq!(
        store.resolve(&addr("sales")).unwrap(),
        Some(addr("bob")),
        "explicit alias must win over the catch-all"
    );

    // 3. it does not rescue a deliberate alias with a dead target —
    //    that is a config error to surface, not to paper over
    store.upsert(&addr("contact"), &addr("gone")).unwrap();
    assert_eq!(
        store.resolve(&addr("contact")).unwrap(),
        Some(addr("gone")),
        "catch-all must not capture a later hop"
    );

    // 4. it is scoped to its own domain
    assert_eq!(
        store.resolve("someone@other-domain.test").unwrap(),
        None,
        "catch-all must not answer for other domains"
    );
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn memory_store_honours_the_catch_all_contract() {
        assert_catch_all_contract(&MemoryAliasStore::default());
    }
}
