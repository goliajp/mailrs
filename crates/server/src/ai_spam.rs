use redis::AsyncCommands;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// AI classification result
#[derive(Debug, Clone)]
pub struct AiSpamResult {
    pub score: f64,
    pub reason: String,
}

/// classify a message using the self-hosted LLM
/// only called in the grey zone (1.0 < rule_score < threshold)
/// returns additional score adjustment + reason, or None on failure/timeout
pub async fn classify(
    llm_url: &str,
    valkey: Option<&redis::aio::ConnectionManager>,
    sender: &str,
    subject: &str,
    body_preview: &str,
) -> Option<AiSpamResult> {
    // check Valkey cache first
    let cache_key = make_cache_key(sender, subject, body_preview);
    if let Some(mut vk) = valkey.cloned() {
        if let Ok(Some(cached)) = vk.get::<_, Option<String>>(&cache_key).await {
            if let Some(result) = parse_cached(&cached) {
                tracing::debug!(event = "ai_spam_cache_hit", key = %cache_key);
                return Some(result);
            }
        }
    }

    let system = "You are a spam classifier. Analyze emails and respond with ONLY a JSON object: {\"score\": <0.0-10.0>, \"reason\": \"<brief reason>\"}. Score guide: 0=clearly legitimate, 5=suspicious, 10=obvious spam";

    let user_message = format!(
        "Sender: {sender}\nSubject: {subject}\nBody preview: {body_preview}"
    );

    let client = reqwest::Client::new();
    let api_key = std::env::var("MAILRS_LLM_API_KEY").ok();
    let response = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        {
            let mut req = client
                .post(llm_url)
                .header("x-caller", "mailrs")
                .json(&serde_json::json!({
                    "system": system,
                    "messages": [{"role": "user", "content": user_message}],
                    "temperature": 0.1
                }));
            if let Some(ref key) = api_key {
                req = req.header("Authorization", format!("Bearer {key}"));
            }
            req.send()
        },
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            tracing::warn!(event = "ai_spam_error", error = %e, "LLM request failed");
            return None;
        }
        Err(_) => {
            tracing::warn!(event = "ai_spam_timeout", "LLM request timed out (10s)");
            return None;
        }
    };

    let body = match response.json::<serde_json::Value>().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(event = "ai_spam_parse_error", error = %e);
            return None;
        }
    };

    let text = body["content"].as_str().unwrap_or("");
    let result = parse_ai_response(text)?;

    // cache result in Valkey (24h TTL)
    if let Some(mut vk) = valkey.cloned() {
        let cached = serde_json::json!({"s": result.score, "r": result.reason}).to_string();
        let _: Result<(), _> = vk.set_ex(&cache_key, &cached, 86400).await;
    }

    tracing::info!(
        event = "ai_spam_classified",
        score = result.score,
        reason = %result.reason,
        "AI spam classification complete"
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
        let r = parse_ai_response(r#"Here is my analysis: {"score": 2.0, "reason": "legitimate newsletter"} hope that helps"#).unwrap();
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
