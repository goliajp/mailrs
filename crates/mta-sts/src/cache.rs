//! Policy cache trait + in-memory reference implementation.
//!
//! A real STS-aware MTA caches the parsed policy per `(domain, id)`
//! pair and re-fetches only when:
//!
//! - the TXT record's `id` field changes (caller compares before
//!   calling `Cache::get`), OR
//! - the cached policy's `max_age` has elapsed (caller's clock).
//!
//! This crate defines the [`Cache`] async trait; we ship
//! [`InMemoryCache`] as a `tokio::sync::RwLock<HashMap>`-backed ref
//! impl suitable for single-process deployments. Distributed setups
//! plug their own Redis / Memcached store into the trait.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::policy::Policy;

/// One entry's worth of cached policy state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPolicy {
    /// `id` field from the most recent TXT record. Cache key for
    /// freshness comparison.
    ///
    /// **v2 change**: `CompactString` — matches `StsRecord.id`.
    pub id: compact_str::CompactString,
    /// Parsed policy body.
    pub policy: Policy,
    /// Unix-seconds at which the caller fetched the policy file.
    /// Combined with `policy.max_age` this gives an expiry the
    /// caller checks against `now()`.
    pub fetched_at_unix_secs: u64,
}

/// Pluggable cache for parsed STS policies.
///
/// Implementors decide storage (memory, Kevy, etc.). The trait is
/// intentionally small — `get`, `put`, `delete` — so it composes with
/// any KV store.
#[async_trait]
pub trait Cache: Send + Sync {
    /// Read the cached policy for `recipient_domain`. Returns `None`
    /// if there's no entry (caller must do the full TXT + HTTPS
    /// dance).
    async fn get(&self, recipient_domain: &str) -> Option<CachedPolicy>;
    /// Insert or update the cache entry for `recipient_domain`.
    async fn put(&self, recipient_domain: &str, entry: CachedPolicy);
    /// Drop the cache entry (e.g. on `mode: none` policy or a
    /// confirmed expiry).
    async fn delete(&self, recipient_domain: &str);
}

/// In-memory `RwLock<HashMap>` cache for single-process deployments.
///
/// Lookups are non-blocking; writes take the write lock briefly.
/// For high-cardinality deployments swap for a sharded or
/// out-of-process store.
#[derive(Debug, Clone, Default)]
pub struct InMemoryCache {
    inner: Arc<RwLock<HashMap<String, CachedPolicy>>>,
}

impl InMemoryCache {
    /// Construct an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the current size — useful for ops dashboards.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// True iff no entries.
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

#[async_trait]
impl Cache for InMemoryCache {
    async fn get(&self, recipient_domain: &str) -> Option<CachedPolicy> {
        let key = recipient_domain.trim().to_ascii_lowercase();
        self.inner.read().await.get(&key).cloned()
    }
    async fn put(&self, recipient_domain: &str, entry: CachedPolicy) {
        let key = recipient_domain.trim().to_ascii_lowercase();
        self.inner.write().await.insert(key, entry);
    }
    async fn delete(&self, recipient_domain: &str) {
        let key = recipient_domain.trim().to_ascii_lowercase();
        self.inner.write().await.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Policy, PolicyMode};

    fn sample_policy() -> Policy {
        Policy {
            mode: PolicyMode::Enforce,
            mx: vec!["mail.example.com".into()],
            max_age: 86400,
        }
    }

    #[tokio::test]
    async fn in_memory_cache_round_trips() {
        let cache = InMemoryCache::new();
        let entry = CachedPolicy {
            id: "20200101".into(),
            policy: sample_policy(),
            fetched_at_unix_secs: 1_700_000_000,
        };
        cache.put("example.com", entry.clone()).await;
        let got = cache.get("example.com").await.unwrap();
        assert_eq!(got.id, "20200101");
        assert_eq!(got.policy.max_age, 86400);
    }

    #[tokio::test]
    async fn in_memory_cache_key_is_case_insensitive() {
        let cache = InMemoryCache::new();
        let entry = CachedPolicy {
            id: "abc".into(),
            policy: sample_policy(),
            fetched_at_unix_secs: 0,
        };
        cache.put("Example.COM", entry.clone()).await;
        assert_eq!(cache.get("example.com").await.unwrap().id, "abc");
        assert_eq!(cache.get("EXAMPLE.COM").await.unwrap().id, "abc");
    }

    #[tokio::test]
    async fn in_memory_cache_delete_removes_entry() {
        let cache = InMemoryCache::new();
        cache
            .put(
                "example.com",
                CachedPolicy {
                    id: "x".into(),
                    policy: sample_policy(),
                    fetched_at_unix_secs: 0,
                },
            )
            .await;
        assert!(cache.get("example.com").await.is_some());
        cache.delete("example.com").await;
        assert!(cache.get("example.com").await.is_none());
    }

    #[tokio::test]
    async fn in_memory_cache_put_overwrites() {
        let cache = InMemoryCache::new();
        cache
            .put(
                "x.com",
                CachedPolicy {
                    id: "v1".into(),
                    policy: sample_policy(),
                    fetched_at_unix_secs: 100,
                },
            )
            .await;
        cache
            .put(
                "x.com",
                CachedPolicy {
                    id: "v2".into(),
                    policy: sample_policy(),
                    fetched_at_unix_secs: 200,
                },
            )
            .await;
        let got = cache.get("x.com").await.unwrap();
        assert_eq!(got.id, "v2");
        assert_eq!(got.fetched_at_unix_secs, 200);
    }

    #[tokio::test]
    async fn in_memory_cache_len_and_is_empty() {
        let cache = InMemoryCache::new();
        assert!(cache.is_empty().await);
        assert_eq!(cache.len().await, 0);
        cache
            .put(
                "x.com",
                CachedPolicy {
                    id: "v".into(),
                    policy: sample_policy(),
                    fetched_at_unix_secs: 0,
                },
            )
            .await;
        assert!(!cache.is_empty().await);
        assert_eq!(cache.len().await, 1);
    }
}
