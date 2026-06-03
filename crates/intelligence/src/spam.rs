//! Spam classification via [`LlmProvider`] with an optional cache.
//!
//! [`classify`] hashes `(sender, subject, body_preview)` into a cache key
//! and consults the optional [`SpamCache`] before calling the provider.
//! On cache miss, the result is written back with a 24-hour TTL.
//!
//! A Kevy-backed [`SpamCache`] implementation ([`KevySpamCache`]) is
//! available under the default `kevy-cache` feature.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use async_trait::async_trait;

use crate::provider::LlmProvider;

/// AI spam classification result.
#[derive(Debug, Clone)]
pub struct AiSpamResult {
    /// 0.0 (clearly legitimate) → 10.0 (obvious spam).
    pub score: f64,
    /// Short natural-language reason from the model.
    pub reason: String,
}

/// Pluggable cache for spam classification results.
///
/// Implementations should ignore failures rather than propagate them: a
/// cache miss is always recoverable by re-asking the provider.
#[async_trait]
pub trait SpamCache: Send + Sync {
    /// Look up a cached result by key. Return `None` on miss or error.
    async fn get(&self, key: &str) -> Option<String>;
    /// Store a result with TTL (seconds). Errors are ignored.
    async fn set(&self, key: &str, value: &str, ttl_secs: u64);
}

/// Classify a message using `provider`, consulting `cache` if supplied.
///
/// Designed for the "grey zone" between rule-based spam thresholds —
/// callers typically only invoke this when their cheaper heuristics are
/// undecided. Returns `None` on provider failure or unparseable response.
pub async fn classify(
    provider: &dyn LlmProvider,
    cache: Option<&dyn SpamCache>,
    sender: &str,
    subject: &str,
    body_preview: &str,
) -> Option<AiSpamResult> {
    let cache_key = make_cache_key(sender, subject, body_preview);

    if let Some(cache) = cache
        && let Some(cached) = cache.get(&cache_key).await
        && let Some(result) = parse_cached(&cached)
    {
        tracing::debug!(event = "ai_spam_cache_hit", key = %cache_key);
        return Some(result);
    }

    let system = "You are a spam classifier. Analyze emails and respond with ONLY a JSON object: {\"score\": <0.0-10.0>, \"reason\": \"<brief reason>\"}. Score guide: 0=clearly legitimate, 5=suspicious, 10=obvious spam";

    let user_message =
        format!("Sender: {sender}\nSubject: {subject}\nBody preview: {body_preview}");

    let text = provider.complete(system, &user_message, 0.1).await?;
    let result = parse_ai_response(&text)?;

    if let Some(cache) = cache {
        let cached = serde_json::json!({"s": result.score, "r": result.reason}).to_string();
        cache.set(&cache_key, &cached, 86400).await;
    }

    tracing::info!(
        event = "ai_spam_classified",
        score = result.score,
        reason = %result.reason,
    );

    Some(result)
}

fn make_cache_key(sender: &str, subject: &str, body_preview: &str) -> String {
    let mut hasher = DefaultHasher::new();
    sender.hash(&mut hasher);
    subject.hash(&mut hasher);
    body_preview.hash(&mut hasher);
    let hash = hasher.finish();
    format!("ai:{hash:x}")
}

fn parse_cached(s: &str) -> Option<AiSpamResult> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    let score = v["s"].as_f64()?;
    let reason = v["r"].as_str().unwrap_or("").to_string();
    Some(AiSpamResult { score, reason })
}

fn parse_ai_response(text: &str) -> Option<AiSpamResult> {
    let start = text.find('{')?;
    let end = text.rfind('}')? + 1;
    let json_str = &text[start..end];
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let score = v["score"].as_f64()?;
    let reason = v["reason"].as_str().unwrap_or("").to_string();
    Some(AiSpamResult {
        score: score.clamp(0.0, 10.0),
        reason,
    })
}

#[cfg(feature = "kevy-cache")]
pub use kevy_impl::KevySpamCache;

#[cfg(feature = "kevy-cache")]
mod kevy_impl {
    use std::time::Duration;

    use async_trait::async_trait;
    use kevy_embedded::Store;

    use super::SpamCache;

    /// Kevy-backed [`SpamCache`] using an in-process [`kevy_embedded::Store`].
    ///
    /// The cache silently ignores all store errors — a missing/failed
    /// lookup always falls through to the provider, and a failed `set`
    /// just loses one cache entry. Both situations are recoverable
    /// without breaking classification.
    #[derive(Clone)]
    pub struct KevySpamCache {
        store: Store,
    }

    impl std::fmt::Debug for KevySpamCache {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("KevySpamCache").finish_non_exhaustive()
        }
    }

    impl KevySpamCache {
        /// Construct a Kevy-backed spam-classification cache from an
        /// in-process [`Store`] handle (callers typically pass a clone of
        /// the shared cement-owned store).
        pub fn new(store: Store) -> Self {
            Self { store }
        }
    }

    #[async_trait]
    impl SpamCache for KevySpamCache {
        async fn get(&self, key: &str) -> Option<String> {
            self.store
                .get(key.as_bytes())
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        }

        async fn set(&self, key: &str, value: &str, ttl_secs: u64) {
            let _ = self.store.set_with_ttl(
                key.as_bytes(),
                value.as_bytes(),
                Duration::from_secs(ttl_secs),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ai_response_valid() {
        let r = parse_ai_response(r#"{"score": 7.5, "reason": "phishing attempt"}"#).unwrap();
        assert!((r.score - 7.5).abs() < 0.01);
        assert_eq!(r.reason, "phishing attempt");
    }

    #[test]
    fn parse_ai_response_with_surrounding_text() {
        let r = parse_ai_response(
            r#"Here is my analysis: {"score": 2.0, "reason": "legitimate newsletter"} hope that helps"#,
        )
        .unwrap();
        assert!((r.score - 2.0).abs() < 0.01);
    }

    #[test]
    fn parse_ai_response_invalid() {
        assert!(parse_ai_response("no json here").is_none());
        assert!(parse_ai_response(r#"{"no_score": true}"#).is_none());
    }

    #[test]
    fn parse_ai_response_clamps_score() {
        let r = parse_ai_response(r#"{"score": 15.0, "reason": "very spam"}"#).unwrap();
        assert!((r.score - 10.0).abs() < 0.01);
    }

    #[test]
    fn cache_key_format() {
        let key = make_cache_key("user@example.com", "Hello World", "body");
        assert!(key.starts_with("ai:"));
        let key2 = make_cache_key("other@example.com", "Hello World", "body");
        assert_ne!(key, key2);
    }

    #[test]
    fn parse_cached_roundtrip() {
        let cached = r#"{"s":7.5,"r":"phishing attempt"}"#;
        let r = parse_cached(cached).unwrap();
        assert!((r.score - 7.5).abs() < 0.01);
        assert_eq!(r.reason, "phishing attempt");
    }

    #[test]
    fn parse_cached_with_pipe_in_reason() {
        let cached = r#"{"s":3.0,"r":"too many links | phishing indicators"}"#;
        let r = parse_cached(cached).unwrap();
        assert!((r.score - 3.0).abs() < 0.01);
        assert_eq!(r.reason, "too many links | phishing indicators");
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::provider::LlmProvider;
    use std::sync::Mutex;

    /// LlmProvider returning a canned response, useful for asserting
    /// downstream behavior without touching a real LLM endpoint.
    struct MockProvider {
        canned: String,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn complete(&self, _system: &str, _user: &str, _temp: f32) -> Option<String> {
            *self.calls.lock().unwrap() += 1;
            Some(self.canned.clone())
        }
        async fn embed(&self, _text: &str) -> Option<Vec<f32>> {
            None
        }
        fn model_id(&self) -> &str {
            "mock/1"
        }
    }

    /// LlmProvider that always errors.
    struct DeadProvider;

    #[async_trait]
    impl LlmProvider for DeadProvider {
        async fn complete(&self, _system: &str, _user: &str, _temp: f32) -> Option<String> {
            None
        }
        async fn embed(&self, _text: &str) -> Option<Vec<f32>> {
            None
        }
        fn model_id(&self) -> &str {
            "dead/0"
        }
    }

    /// In-memory SpamCache for testing the cache pathway end-to-end.
    struct MemCache {
        inner: Mutex<std::collections::HashMap<String, String>>,
    }

    #[async_trait]
    impl SpamCache for MemCache {
        async fn get(&self, key: &str) -> Option<String> {
            self.inner.lock().unwrap().get(key).cloned()
        }
        async fn set(&self, key: &str, value: &str, _ttl: u64) {
            self.inner.lock().unwrap().insert(key.into(), value.into());
        }
    }

    #[tokio::test]
    async fn classify_returns_score_from_provider() {
        let provider = MockProvider {
            canned: r#"{"score": 7.5, "reason": "phishing pattern"}"#.into(),
            calls: Mutex::new(0),
        };
        let result = classify(&provider, None, "evil@x", "Win now!", "click here")
            .await
            .expect("classify must succeed");
        assert!((result.score - 7.5).abs() < 0.01);
        assert_eq!(result.reason, "phishing pattern");
    }

    #[tokio::test]
    async fn classify_returns_none_on_dead_provider() {
        let result = classify(&DeadProvider, None, "any@x", "subj", "body").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn classify_unparseable_response_returns_none() {
        let provider = MockProvider {
            canned: "I don't speak JSON".into(),
            calls: Mutex::new(0),
        };
        assert!(classify(&provider, None, "x", "y", "z").await.is_none());
    }

    #[tokio::test]
    async fn classify_writes_to_cache_on_miss() {
        let provider = MockProvider {
            canned: r#"{"score": 2.0, "reason": "legit"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };
        let _ = classify(&provider, Some(&cache), "a", "b", "c").await;
        assert_eq!(
            cache.inner.lock().unwrap().len(),
            1,
            "cache must hold one entry"
        );
    }

    #[tokio::test]
    async fn classify_hits_cache_on_second_call() {
        let provider = MockProvider {
            canned: r#"{"score": 5.0, "reason": "borderline"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };

        let r1 = classify(&provider, Some(&cache), "a", "b", "c")
            .await
            .unwrap();
        let r2 = classify(&provider, Some(&cache), "a", "b", "c")
            .await
            .unwrap();

        assert!((r1.score - 5.0).abs() < 0.01);
        assert!((r2.score - 5.0).abs() < 0.01);
        assert_eq!(
            *provider.calls.lock().unwrap(),
            1,
            "second call should hit cache, not provider"
        );
    }

    #[tokio::test]
    async fn classify_different_inputs_use_different_cache_keys() {
        let provider = MockProvider {
            canned: r#"{"score": 3.0, "reason": "ok"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };

        classify(&provider, Some(&cache), "a", "subject1", "body").await;
        classify(&provider, Some(&cache), "a", "subject2", "body").await;

        assert_eq!(cache.inner.lock().unwrap().len(), 2);
        assert_eq!(*provider.calls.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn classify_caches_negative_decisions_too() {
        let provider = MockProvider {
            canned: r#"{"score": 0.1, "reason": "totally legit"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };
        classify(&provider, Some(&cache), "a", "b", "c").await;
        assert_eq!(
            cache.inner.lock().unwrap().len(),
            1,
            "low-score result still cached"
        );
    }

    // ===== Additional integration tests =====

    #[tokio::test]
    async fn classify_handles_empty_strings() {
        // Edge case: empty sender/subject/body should still produce a cache key
        // (the hasher tolerates empty input) and round-trip through the provider.
        let provider = MockProvider {
            canned: r#"{"score": 0.0, "reason": "empty"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };
        let r = classify(&provider, Some(&cache), "", "", "").await.unwrap();
        assert!((r.score - 0.0).abs() < 0.01);
        assert_eq!(cache.inner.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn classify_cache_corrupted_value_falls_through_to_provider() {
        // If the cached value is malformed, classify should fall through and
        // ask the provider, then overwrite the cached value.
        let provider = MockProvider {
            canned: r#"{"score": 4.0, "reason": "fresh"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };
        // pre-seed cache with garbage that has the correct key
        let key = make_cache_key("a", "b", "c");
        cache
            .inner
            .lock()
            .unwrap()
            .insert(key.clone(), "not-json".to_string());
        let r = classify(&provider, Some(&cache), "a", "b", "c")
            .await
            .unwrap();
        assert!((r.score - 4.0).abs() < 0.01, "fell through to provider");
        // and the cache was overwritten with a valid entry
        let cached = cache.inner.lock().unwrap().get(&key).cloned().unwrap();
        assert!(cached.contains("\"s\":4.0") || cached.contains("\"s\":4"));
    }

    #[tokio::test]
    async fn classify_subject_change_busts_cache() {
        let provider = MockProvider {
            canned: r#"{"score": 1.0, "reason": "ok"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache {
            inner: Mutex::new(Default::default()),
        };
        classify(&provider, Some(&cache), "a", "subj1", "body").await;
        classify(&provider, Some(&cache), "a", "subj2", "body").await;
        // Subject change must produce a distinct cache key -> provider called twice
        assert_eq!(*provider.calls.lock().unwrap(), 2);
        assert_eq!(cache.inner.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn classify_returns_none_when_score_field_missing() {
        // Response with a "reason" but no "score" must fail parsing.
        let provider = MockProvider {
            canned: r#"{"reason": "I forgot the score"}"#.into(),
            calls: Mutex::new(0),
        };
        let r = classify(&provider, None, "a", "b", "c").await;
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn classify_score_clamped_after_provider() {
        // If provider returns out-of-range score (e.g. 15.0), it must be clamped to 10.0.
        let provider = MockProvider {
            canned: r#"{"score": 99.9, "reason": "off the scale"}"#.into(),
            calls: Mutex::new(0),
        };
        let r = classify(&provider, None, "a", "b", "c").await.unwrap();
        assert!((r.score - 10.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn classify_negative_score_clamped() {
        let provider = MockProvider {
            canned: r#"{"score": -5.0, "reason": "negative"}"#.into(),
            calls: Mutex::new(0),
        };
        let r = classify(&provider, None, "a", "b", "c").await.unwrap();
        assert!((r.score - 0.0).abs() < 0.01);
    }
}
