use serde::{Deserialize, Serialize};

/// current prompt version — bump this to trigger automatic reanalysis of all messages
pub const PROMPT_VERSION: &str = "v5";

/// self-hosted LLM configuration
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub url: String,
    pub client: reqwest::Client,
}

impl LlmConfig {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::new(),
        }
    }

    /// model_version string stored in DB — includes prompt version
    pub fn model_version(&self) -> String {
        format!("qwen/{PROMPT_VERSION}")
    }
}

/// full analysis result from AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAnalysis {
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: Vec<PersonMention>,
    pub dates: Vec<DateMention>,
    pub amounts: Vec<AmountMention>,
    pub action_items: Vec<String>,
    #[serde(default)]
    pub clean_text: String,
    #[serde(default)]
    pub requires_action: bool,
    #[serde(default)]
    pub sender_intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_deadline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonMention {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateMention {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iso_date: Option<String>,
    #[serde(default)]
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmountMention {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default)]
    pub context: String,
}

/// truncate a string at a char boundary, never splitting a multi-byte character
fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// generate a 1024-dimensional embedding using self-hosted qwen3-embedding
pub async fn generate_embedding(config: &LlmConfig, text: &str) -> Option<Vec<f32>> {
    // derive embed URL from LLM URL: replace /complete with /embed
    let embed_url = config.url.replace("/complete", "/embed");

    let body = serde_json::json!({
        "input": text
    });

    let response = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        config.client.post(&embed_url).json(&body).send(),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("embedding request error: {e}");
            return None;
        }
        Err(_) => {
            eprintln!("embedding request timeout (30s)");
            return None;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        eprintln!(
            "embedding API error {status}: {}",
            &body[..body.len().min(200)]
        );
        return None;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("embedding response parse error: {e}");
            return None;
        }
    };

    let values = json["embeddings"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_array())?;

    let embedding: Vec<f32> = values
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    let dims = json["dimensions"].as_u64().unwrap_or(1024) as usize;
    if embedding.len() == dims {
        Some(embedding)
    } else {
        eprintln!(
            "embedding bad dim: {} (expected {})",
            embedding.len(),
            dims
        );
        None
    }
}

/// call the self-hosted LLM API
pub async fn call_llm(
    config: &LlmConfig,
    system: &str,
    user_message: &str,
    temperature: f32,
) -> Option<String> {
    let body = serde_json::json!({
        "system": system,
        "messages": [{"role": "user", "content": user_message}],
        "temperature": temperature
    });

    let response = match tokio::time::timeout(
        std::time::Duration::from_secs(60),
        config.client.post(&config.url).json(&body).send(),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("LLM request error: {e}");
            return None;
        }
        Err(_) => {
            eprintln!("LLM request timeout (60s)");
            return None;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        eprintln!(
            "LLM API error {status}: {}",
            &body[..body.len().min(200)]
        );
        return None;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("LLM response parse error: {e}");
            return None;
        }
    };

    json["content"].as_str().map(|s| s.to_string())
}

/// analyze an email using self-hosted LLM for classification, summarization, and entity extraction
pub async fn analyze_email(
    config: &LlmConfig,
    sender: &str,
    subject: &str,
    body_text: &str,
) -> Option<EmailAnalysis> {
    let body_text = truncate_str(body_text, 8000);

    let system = r#"You are an email analysis assistant. Analyze emails and respond with ONLY a JSON object. No markdown fences, no explanation.

## Category Definitions
- personal: private messages from friends/family/acquaintances
- work: business communications, internal memos, project discussions, meeting invites
- notification: automated system alerts, account notifications, password resets, login alerts
- promotion: marketing emails, sales, coupons, セール, キャンペーン, クーポン, 配信, お得, ポイント, ads
- newsletter: periodic digest emails, blog updates, curated content, メルマガ, ニュースレター
- receipt: purchase confirmations, invoices, order summaries, 注文確認, 領収書
- shipping: delivery tracking, shipment updates, 配送, 発送, お届け
- travel: flight/hotel/rental confirmations, itineraries, boarding passes
- finance: bank statements, investment alerts, payment notices, 入金, 振込
- spam: unsolicited bulk email, unwanted advertising
- scam: phishing attempts, social engineering, credential theft, advance-fee fraud
- general: anything that doesn't fit the above

## Risk Score Guidelines
- 0-10: Trusted — from verified senders, expected content, no suspicious elements
- 11-25: Normal — legitimate marketing/notifications, may have tracking pixels or unsubscribe links
- 26-50: Suspicious — unknown sender, unusual requests, mismatched reply-to, suspicious links
- 51-75: Dangerous — requests for passwords/credit cards/personal info, urgency tactics, domain spoofing
- 76-100: Phishing/Scam — impersonation, fake login pages, malware links, advance-fee fraud

## Action Detection
- requires_action: true if the recipient needs to do something (reply, review, approve, pay, sign, attend, etc.)
- sender_intent: classify the sender's primary purpose: "request", "inform", "confirm", "social", "alert"
- action_deadline: if the email mentions a deadline, extract as ISO 8601 (YYYY-MM-DD). null if none.

## clean_text Instructions
Extract the main readable content. Remove HTML tags, navigation, headers/footers, unsubscribe notices, tracking elements. Max 2000 characters.

## JSON schema
{"category": "<one of the categories above>", "risk_score": <0-100>, "risk_reason": "<brief reason>", "summary": "<2-3 sentence summary>", "clean_text": "<extracted clean text, max 2000 chars>", "requires_action": <true|false>, "sender_intent": "<request|inform|confirm|social|alert>", "action_deadline": "<ISO 8601 date or null>", "people": [{"name": "...", "email": "...", "role": "..."}], "dates": [{"text": "original text", "iso_date": "YYYY-MM-DD", "context": "..."}], "amounts": [{"text": "original text", "value": 123.45, "currency": "USD", "context": "..."}], "action_items": ["<action required by recipient>"]}"#;

    let user_message = format!(
        "Analyze this email:\n\nFrom: {sender}\nSubject: {subject}\nBody:\n{body_text}"
    );

    let text = call_llm(config, system, &user_message, 0.1).await?;
    parse_analysis_response(&text)
}

/// parse the JSON analysis response, handling markdown fences and extra text
fn parse_analysis_response(text: &str) -> Option<EmailAnalysis> {
    // strip markdown code fences if present
    let text = text.trim();
    let text = if let Some(stripped) = text.strip_prefix("```json") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else if let Some(stripped) = text.strip_prefix("```") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else {
        text
    };

    // find JSON object boundaries
    let start = text.find('{')?;
    let end = text.rfind('}')? + 1;
    let json_str = &text[start..end];

    let mut analysis: EmailAnalysis = serde_json::from_str(json_str).ok()?;

    // clamp risk_score
    analysis.risk_score = analysis.risk_score.min(100);

    // truncate clean_text to 2000 chars
    if analysis.clean_text.len() > 2000 {
        let mut end = 2000;
        while end > 0 && !analysis.clean_text.is_char_boundary(end) {
            end -= 1;
        }
        analysis.clean_text.truncate(end);
    }

    Some(analysis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_response() {
        let json = r#"{"category":"personal","risk_score":5,"risk_reason":"from known sender","summary":"A friendly hello","people":[{"name":"John","email":"john@example.com"}],"dates":[],"amounts":[],"action_items":["reply to John"],"clean_text":"Hello there","requires_action":true,"sender_intent":"request","action_deadline":"2026-03-15"}"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.category, "personal");
        assert_eq!(result.risk_score, 5);
        assert_eq!(result.people.len(), 1);
        assert_eq!(result.people[0].name, "John");
        assert_eq!(result.action_items, vec!["reply to John"]);
        assert_eq!(result.clean_text, "Hello there");
        assert!(result.requires_action);
        assert_eq!(result.sender_intent, "request");
        assert_eq!(result.action_deadline.as_deref(), Some("2026-03-15"));
    }

    #[test]
    fn parse_response_without_clean_text() {
        let json = r#"{"category":"personal","risk_score":5,"risk_reason":"safe","summary":"test","people":[],"dates":[],"amounts":[],"action_items":[]}"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.clean_text, "");
        assert!(!result.requires_action);
        assert_eq!(result.sender_intent, "");
        assert_eq!(result.action_deadline, None);
    }

    #[test]
    fn parse_markdown_fenced_response() {
        let json = "```json\n{\"category\":\"spam\",\"risk_score\":80,\"risk_reason\":\"suspicious links\",\"summary\":\"spam email\",\"people\":[],\"dates\":[],\"amounts\":[],\"action_items\":[],\"clean_text\":\"\"}\n```";
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.category, "spam");
        assert_eq!(result.risk_score, 80);
    }

    #[test]
    fn parse_response_with_surrounding_text() {
        let json = "Here is the analysis:\n{\"category\":\"general\",\"risk_score\":0,\"risk_reason\":\"safe\",\"summary\":\"test\",\"people\":[],\"dates\":[],\"amounts\":[],\"action_items\":[],\"clean_text\":\"\"}\nDone.";
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.category, "general");
    }

    #[test]
    fn parse_invalid_response() {
        assert!(parse_analysis_response("no json here").is_none());
        assert!(parse_analysis_response("").is_none());
        assert!(parse_analysis_response("{invalid}").is_none());
    }

    #[test]
    fn risk_score_clamped() {
        let json = r#"{"category":"scam","risk_score":150,"risk_reason":"phishing","summary":"dangerous","people":[],"dates":[],"amounts":[],"action_items":[],"clean_text":""}"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.risk_score, 100);
    }

    #[test]
    fn parse_with_optional_fields() {
        let json = r#"{"category":"work","risk_score":10,"risk_reason":"normal","summary":"meeting invite","people":[{"name":"Alice"}],"dates":[{"text":"March 5th","context":"meeting date"}],"amounts":[{"text":"$500","value":500.0,"currency":"USD","context":"budget"}],"action_items":[],"clean_text":"Meeting on March 5th"}"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.people[0].email, None);
        assert_eq!(result.dates[0].iso_date, None);
        assert_eq!(result.amounts[0].value, Some(500.0));
        assert_eq!(result.amounts[0].currency.as_deref(), Some("USD"));
    }

    #[test]
    fn truncate_multibyte_safe() {
        let s = "あいう";
        assert_eq!(truncate_str(s, 9), "あいう");
        assert_eq!(truncate_str(s, 8), "あい");
        assert_eq!(truncate_str(s, 6), "あい");
        assert_eq!(truncate_str(s, 5), "あ");
        assert_eq!(truncate_str(s, 3), "あ");
        assert_eq!(truncate_str(s, 2), "");
        assert_eq!(truncate_str(s, 0), "");
        assert_eq!(truncate_str("hello", 3), "hel");
    }

    #[test]
    fn model_version_format() {
        let config = LlmConfig::new("http://localhost".into());
        let mv = config.model_version();
        assert!(mv.contains(PROMPT_VERSION));
        assert!(mv.contains("qwen"));
    }
}
