use serde::{Deserialize, Serialize};

/// current prompt version — bump this to trigger automatic reanalysis of all messages
pub const PROMPT_VERSION: &str = "v3";

/// gemini API configuration
#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub api_key: String,
    pub embedding_model: String,
    pub analysis_model: String,
    pub client: reqwest::Client,
}

impl GeminiConfig {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            embedding_model: "gemini-embedding-001".into(),
            analysis_model: "gemini-2.5-flash".into(),
            client: reqwest::Client::new(),
        }
    }

    /// model_version string stored in DB — includes prompt version
    pub fn model_version(&self) -> String {
        format!(
            "{}/{}/{}",
            self.analysis_model, self.embedding_model, PROMPT_VERSION
        )
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

/// generate a 768-dimensional embedding using Gemini text-embedding-004
pub async fn generate_embedding(config: &GeminiConfig, text: &str) -> Option<Vec<f32>> {
    let text = truncate_str(text, 8000);

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent?key={}",
        config.embedding_model, config.api_key
    );

    let body = serde_json::json!({
        "model": format!("models/{}", config.embedding_model),
        "content": {
            "parts": [{"text": text}]
        },
        "outputDimensionality": 768
    });

    let response = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        config.client.post(&url).json(&body).send(),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("gemini embedding error: {e}");
            return None;
        }
        Err(_) => {
            eprintln!("gemini embedding timeout (10s)");
            return None;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        eprintln!(
            "gemini embedding API error {status}: {}",
            &body[..body.len().min(200)]
        );
        return None;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("gemini embedding parse error: {e}");
            return None;
        }
    };

    let values = json["embedding"]["values"].as_array()?;
    let embedding: Vec<f32> = values
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();

    if embedding.len() == 768 {
        Some(embedding)
    } else {
        eprintln!(
            "gemini embedding bad dim: {} (expected 768)",
            embedding.len()
        );
        None
    }
}

/// analyze an email using Gemini for classification, summarization, and entity extraction
pub async fn analyze_email(
    config: &GeminiConfig,
    sender: &str,
    subject: &str,
    body_text: &str,
) -> Option<EmailAnalysis> {
    let body_text = truncate_str(body_text, 8000);

    let prompt = format!(
        r#"You are an email analysis assistant. Analyze this email and respond with ONLY a JSON object. No markdown fences, no explanation.

From: {sender}
Subject: {subject}
Body:
{body_text}

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

## Promotion Detection Signals
Look for: unsubscribe/配信停止/退会 links, tracking pixels, sale/discount language, coupon codes, bulk sender headers, marketing sender names

## Phishing Detection Signals
Look for: urgent calls to action (「至急」「今すぐ」「アカウントが停止」), sender domain mismatch, requests for credentials/payment info, suspicious shortened URLs, display name spoofing

## clean_text Instructions
Extract the main readable content from the email body. Remove all HTML tags, navigation, headers/footers, unsubscribe notices, tracking elements, and boilerplate. Preserve paragraph structure with blank lines. Convert links to markdown format [text](url). Keep the text natural and readable. Max 2000 characters. If the body is already plain text, clean up whitespace and formatting.

## Action Detection
- requires_action: true if the recipient needs to do something (reply, review, approve, pay, sign, attend, etc.)
- sender_intent: classify the sender's primary purpose:
  - "request" — asking the recipient to do something
  - "inform" — sharing information, no action needed
  - "confirm" — confirming a transaction, booking, or agreement
  - "social" — social greeting, introduction, or casual conversation
  - "alert" — urgent notification requiring attention (security, system, billing)
- action_deadline: if the email mentions a deadline or due date for the action, extract as ISO 8601 (YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS). null if none.

JSON schema:
{{
  "category": "<one of the categories above>",
  "risk_score": <0-100>,
  "risk_reason": "<brief reason for risk score>",
  "summary": "<2-3 sentence summary of the email content and purpose>",
  "clean_text": "<extracted clean readable text from the email, max 2000 chars>",
  "requires_action": <true|false>,
  "sender_intent": "<request|inform|confirm|social|alert>",
  "action_deadline": "<ISO 8601 date or null>",
  "people": [{{"name": "...", "email": "...", "role": "..."}}],
  "dates": [{{"text": "original text", "iso_date": "YYYY-MM-DD", "context": "..."}}],
  "amounts": [{{"text": "original text", "value": 123.45, "currency": "USD", "context": "..."}}],
  "action_items": ["<action required by recipient>"]
}}"#
    );

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        config.analysis_model, config.api_key
    );

    let body = serde_json::json!({
        "contents": [{"parts": [{"text": prompt}]}],
        "generationConfig": {
            "temperature": 0.1,
            "maxOutputTokens": 2048
        }
    });

    let response = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        config.client.post(&url).json(&body).send(),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => {
            eprintln!("gemini analysis error: {e}");
            return None;
        }
        Err(_) => {
            eprintln!("gemini analysis timeout (30s)");
            return None;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        eprintln!(
            "gemini analysis API error {status}: {}",
            &body[..body.len().min(200)]
        );
        return None;
    }

    let json: serde_json::Value = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("gemini analysis parse error: {e}");
            return None;
        }
    };

    // extract text from Gemini response
    let text = json["candidates"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|c| c["content"]["parts"].as_array())
        .and_then(|parts| parts.first())
        .and_then(|p| p["text"].as_str())
        .unwrap_or("");

    parse_analysis_response(text)
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
        // defaults for new fields
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
        // 'あ' is 3 bytes in UTF-8
        let s = "あいう"; // 9 bytes total
        assert_eq!(truncate_str(s, 9), "あいう");
        assert_eq!(truncate_str(s, 8), "あい"); // rounds down to 6
        assert_eq!(truncate_str(s, 6), "あい");
        assert_eq!(truncate_str(s, 5), "あ");
        assert_eq!(truncate_str(s, 3), "あ");
        assert_eq!(truncate_str(s, 2), "");
        assert_eq!(truncate_str(s, 0), "");
        // ASCII is fine
        assert_eq!(truncate_str("hello", 3), "hel");
    }

    #[test]
    fn prompt_version_in_model_version() {
        let config = GeminiConfig::new("test-key".into());
        let mv = config.model_version();
        assert!(mv.contains(PROMPT_VERSION));
        assert!(mv.contains("gemini-2.5-flash"));
    }
}
