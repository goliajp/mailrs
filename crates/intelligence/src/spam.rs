//! Spam classification via [`LlmProvider`] with an optional cache.
//!
//! [`classify`] hashes `(sender, subject, body_preview)` into a cache key
//! and consults the optional [`SpamCache`] before calling the provider.
//! On cache miss, the result is written back with a 24-hour TTL.
//!
//! A Redis-backed [`SpamCache`] implementation ([`RedisSpamCache`]) is
//! available under the default `redis-cache` feature.

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

#[cfg(feature = "redis-cache")]
pub use redis_impl::RedisSpamCache;

#[cfg(feature = "redis-cache")]
mod redis_impl {
    use super::SpamCache;
    use async_trait::async_trait;
    use redis::AsyncCommands;

    /// Redis-backed [`SpamCache`] using a shared [`redis::aio::ConnectionManager`].
    ///
    /// The cache silently ignores all Redis errors — a missing/failed
    /// cache lookup always falls through to the provider, and a failed
    /// `set` just loses one cache entry. Both situations are recoverable
    /// without breaking classification.
    #[derive(Debug, Clone)]
    pub struct RedisSpamCache {
        conn: redis::aio::ConnectionManager,
    }

    impl RedisSpamCache {
        pub fn new(conn: redis::aio::ConnectionManager) -> Self {
            Self { conn }
        }
    }

    #[async_trait]
    impl SpamCache for RedisSpamCache {
        async fn get(&self, key: &str) -> Option<String> {
            let mut conn = self.conn.clone();
            conn.get::<_, Option<String>>(key).await.ok().flatten()
        }

        async fn set(&self, key: &str, value: &str, ttl_secs: u64) {
            let mut conn = self.conn.clone();
            let _: Result<(), _> = conn.set_ex(key, value, ttl_secs).await;
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
        let cache = MemCache { inner: Mutex::new(Default::default()) };
        let _ = classify(&provider, Some(&cache), "a", "b", "c").await;
        assert_eq!(cache.inner.lock().unwrap().len(), 1, "cache must hold one entry");
    }

    #[tokio::test]
    async fn classify_hits_cache_on_second_call() {
        let provider = MockProvider {
            canned: r#"{"score": 5.0, "reason": "borderline"}"#.into(),
            calls: Mutex::new(0),
        };
        let cache = MemCache { inner: Mutex::new(Default::default()) };

        let r1 = classify(&provider, Some(&cache), "a", "b", "c").await.unwrap();
        let r2 = classify(&provider, Some(&cache), "a", "b", "c").await.unwrap();

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
        let cache = MemCache { inner: Mutex::new(Default::default()) };

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
        let cache = MemCache { inner: Mutex::new(Default::default()) };
        classify(&provider, Some(&cache), "a", "b", "c").await;
        assert_eq!(cache.inner.lock().unwrap().len(), 1, "low-score result still cached");
    }
}
