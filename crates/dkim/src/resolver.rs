//! DNS resolver trait for DKIM public-key TXT lookups.
//!
//! DKIM verifiers need exactly one DNS query type: TXT at
//! `<selector>._domainkey.<domain>`. We define a minimal trait so
//! callers plug in their own DNS layer.

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::DkimError;

/// Minimal DNS interface — DKIM only needs TXT lookups.
///
/// Implementors map NXDOMAIN to `Ok(vec![])` (the caller maps that
/// to [`DkimResult::PermError`] per RFC 6376 §6.1.2). Reserve
/// `Err(DkimError::DnsTempError)` for actual lookup failures.
///
/// The trait also provides a default [`lookup_public_key`] method
/// composed on top of [`lookup_txt`]; cached resolvers (such as
/// [`CachedDkimResolver`]) override it to skip the per-call TXT
/// parsing + base64 decode on cache hits.
#[async_trait]
pub trait DkimResolver: Send + Sync {
    /// TXT records for `domain`. For DKIM, the caller passes
    /// `<selector>._domainkey.<signing-domain>`.
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DkimError>;

    /// Resolve the **already-extracted DKIM public key bytes** for a
    /// `<selector>._domainkey.<domain>` pair. The default
    /// implementation chains `lookup_txt` → find the record with `p=`
    /// → [`crate::crypto::extract_public_key`].
    ///
    /// Override on caching implementations to skip the per-call TXT
    /// scan + base64 decode (the bytes returned are PKCS8 DER SPKI
    /// for RSA / raw 32 B for Ed25519, ready to feed to
    /// [`crate::crypto::verify_signature`]).
    async fn lookup_public_key(
        &self,
        selector: &str,
        domain: &str,
    ) -> Result<Arc<Vec<u8>>, DkimError> {
        let qname = format!("{selector}._domainkey.{domain}");
        let txts = self.lookup_txt(&qname).await?;
        if txts.is_empty() {
            return Err(DkimError::DnsPermError(format!("no TXT at {qname}")));
        }
        let key_txt = txts
            .iter()
            .find(|s| s.contains("p="))
            .ok_or_else(|| DkimError::InvalidKey("no p= tag in TXT".into()))?;
        Ok(Arc::new(crate::crypto::extract_public_key(key_txt)?))
    }
}

/// Wrapping resolver that caches extracted DKIM public-key bytes
/// per `(selector, domain)`. Caches the post-`extract_public_key`
/// output so cached `lookup_public_key` calls skip both the TXT
/// parse and the base64 decode that the inner resolver would do.
///
/// Real DNS TTLs vary widely (DKIM TXT records often publish 1-24 h
/// TTL); the cache uses a single fixed TTL to bound memory and to
/// avoid honouring an unintentionally-long publisher TTL after a
/// key rotation. Default is **5 minutes**, which is short enough
/// to pick up an emergency key rotation within minutes and long
/// enough to amortize the per-call extract cost across the bursty
/// inbound shape mailrs sees (most messages in a burst come from
/// the same handful of high-volume senders).
///
/// Concurrency: the cache is a `parking_lot::Mutex<HashMap<…>>`-shape
/// (`Arc<std::sync::Mutex<HashMap>>` here to keep zero extra deps);
/// reads + writes are O(1); under contention this is the bottleneck
/// rather than the cache lookup itself.
pub struct CachedDkimResolver<R> {
    inner: R,
    cache: Arc<std::sync::Mutex<std::collections::HashMap<String, CacheEntry>>>,
    ttl: std::time::Duration,
    max_entries: usize,
}

#[derive(Clone)]
struct CacheEntry {
    key_bytes: Arc<Vec<u8>>,
    expires_at: std::time::Instant,
}

impl<R: DkimResolver> CachedDkimResolver<R> {
    /// Default: 5-minute TTL, 512 max entries (room for the long tail
    /// of real-world signers without unbounded growth).
    pub fn new(inner: R) -> Self {
        Self::with_ttl_and_capacity(inner, std::time::Duration::from_secs(300), 512)
    }

    /// Construct with explicit TTL and capacity.
    pub fn with_ttl_and_capacity(inner: R, ttl: std::time::Duration, max_entries: usize) -> Self {
        Self {
            inner,
            cache: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            ttl,
            max_entries,
        }
    }

    /// Drop all cached entries — useful after a publisher key rotation
    /// when an immediate refresh is wanted without waiting out the TTL.
    pub fn invalidate(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

#[async_trait]
impl<R: DkimResolver> DkimResolver for CachedDkimResolver<R> {
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DkimError> {
        // TXT-level lookups are NOT cached at this layer — the caller
        // is asking for the raw TXT records, which would defeat the
        // point of caching the extracted key. Forward to the inner.
        self.inner.lookup_txt(domain).await
    }

    async fn lookup_public_key(
        &self,
        selector: &str,
        domain: &str,
    ) -> Result<Arc<Vec<u8>>, DkimError> {
        let cache_key = format!("{selector}._domainkey.{domain}");
        let now = std::time::Instant::now();

        // Fast path: cache hit.
        if let Ok(cache) = self.cache.lock()
            && let Some(entry) = cache.get(&cache_key)
            && entry.expires_at > now
        {
            return Ok(entry.key_bytes.clone());
        }

        // Miss — perform the underlying lookup + extract through the
        // default trait impl on the inner resolver.
        let key_bytes = self.inner.lookup_public_key(selector, domain).await?;

        // Insert into the cache. Simple capacity bound: when we hit
        // `max_entries`, drop a random entry rather than tracking
        // LRU order (DKIM key sets are bursty around a few high-
        // volume senders so a random eviction approximates LRU on
        // the working-set tail well enough).
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= self.max_entries
                && let Some(k) = cache.keys().next().cloned()
            {
                cache.remove(&k);
            }
            cache.insert(
                cache_key,
                CacheEntry {
                    key_bytes: key_bytes.clone(),
                    expires_at: now + self.ttl,
                },
            );
        }

        Ok(key_bytes)
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock resolver that records how many lookup_txt invocations
    /// the wrapping cache lets through.
    struct CountingResolver {
        calls: AtomicUsize,
        // The TXT to return for any domain (matches the DKIM
        // canonical shape so the default lookup_public_key chain
        // works end-to-end).
        canned_txt: String,
    }

    #[async_trait]
    impl DkimResolver for CountingResolver {
        async fn lookup_txt(&self, _domain: &str) -> Result<Vec<String>, DkimError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![self.canned_txt.clone()])
        }
    }

    fn fixture() -> CountingResolver {
        CountingResolver {
            calls: AtomicUsize::new(0),
            // Real-shape TXT with a tiny inline base64 key.
            canned_txt: "v=DKIM1; k=rsa; p=YWJjZA==".to_string(),
        }
    }

    #[tokio::test]
    async fn cache_hit_avoids_inner_call() {
        let inner = fixture();
        let cached = CachedDkimResolver::new(inner);

        let first = cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        let second = cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        assert_eq!(*first, *second);
        // Inner should have been touched exactly once.
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn distinct_keys_dont_collide() {
        let inner = fixture();
        let cached = CachedDkimResolver::new(inner);
        cached.lookup_public_key("sel1", "a.com").await.unwrap();
        cached.lookup_public_key("sel2", "a.com").await.unwrap();
        cached.lookup_public_key("sel1", "b.com").await.unwrap();
        cached.lookup_public_key("sel1", "a.com").await.unwrap(); // hit
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn ttl_expiry_refreshes() {
        let inner = fixture();
        let cached = CachedDkimResolver::with_ttl_and_capacity(
            inner,
            std::time::Duration::from_millis(20),
            128,
        );
        cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalidate_clears_entries() {
        let inner = fixture();
        let cached = CachedDkimResolver::new(inner);
        cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        cached.invalidate();
        cached
            .lookup_public_key("mail", "example.com")
            .await
            .unwrap();
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn lookup_txt_pass_through_is_not_cached() {
        // Confirms the explicit design choice: the cache layer caches
        // only the extracted-key path. Raw lookup_txt continues to hit
        // the inner resolver on every call.
        let inner = fixture();
        let cached = CachedDkimResolver::new(inner);
        cached.lookup_txt("a.com").await.unwrap();
        cached.lookup_txt("a.com").await.unwrap();
        cached.lookup_txt("a.com").await.unwrap();
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn capacity_evicts_on_overflow() {
        let inner = fixture();
        let cached =
            CachedDkimResolver::with_ttl_and_capacity(inner, std::time::Duration::from_secs(60), 2);
        cached.lookup_public_key("a", "x.com").await.unwrap();
        cached.lookup_public_key("b", "x.com").await.unwrap();
        cached.lookup_public_key("c", "x.com").await.unwrap(); // forces eviction
        // After 3 misses + 1 eviction at the second-to-last insert,
        // inner has been called exactly 3 times.
        assert_eq!(cached.inner.calls.load(Ordering::SeqCst), 3);
    }
}

/// Ready-made [`DkimResolver`] over `hickory_resolver::TokioResolver`.
/// Enabled by the default `hickory` feature.
#[cfg(feature = "hickory")]
pub mod hickory {
    use super::*;
    use hickory_resolver::TokioResolver;
    use hickory_resolver::proto::rr::RData;

    /// Wrap a `TokioResolver` for use as a [`DkimResolver`].
    pub struct HickoryDkimResolver {
        inner: TokioResolver,
    }

    impl HickoryDkimResolver {
        /// Construct from an existing `TokioResolver`.
        pub fn new(resolver: TokioResolver) -> Self {
            Self { inner: resolver }
        }
    }

    /// hickory signals "no records found" via its error message; the
    /// safest cross-version check is the error text.
    fn is_no_records<E: std::fmt::Display>(e: &E) -> bool {
        let s = e.to_string();
        s.contains("no record")
            || s.contains("NXDOMAIN")
            || s.contains("no records found")
            || s.contains("NoRecordsFound")
    }

    #[async_trait]
    impl DkimResolver for HickoryDkimResolver {
        async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DkimError> {
            match self.inner.txt_lookup(domain).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::TXT(txt) = &record.data {
                            out.push(txt.to_string());
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DkimError::DnsTempError(e.to_string())),
            }
        }
    }
}
