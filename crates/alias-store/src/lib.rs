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
        for _ in 0..MAX_HOPS {
            match g.get(&current) {
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
