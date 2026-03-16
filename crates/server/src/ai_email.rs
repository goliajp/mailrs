use serde::{Deserialize, Serialize};

/// current prompt version — bump this to trigger automatic reanalysis of all messages
pub const PROMPT_VERSION: &str = "v7";

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

/// generate a 1024-dimensional embedding using self-hosted qwen3-embedding (with 429 retry)
pub async fn generate_embedding(config: &LlmConfig, text: &str) -> Option<Vec<f32>> {
    let embed_url = config.url.replace("/complete", "/embed");
    let body = serde_json::json!({ "input": text });

    for attempt in 0..3u32 {
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

        if response.status().as_u16() == 429 {
            let wait = if attempt < 2 { 15 } else { 30 };
            eprintln!("embedding rate limited (429), retrying in {wait}s (attempt {})", attempt + 1);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            continue;
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("embedding API error {status}: {}", &text[..text.len().min(200)]);
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
        return if embedding.len() == dims {
            Some(embedding)
        } else {
            eprintln!("embedding bad dim: {} (expected {})", embedding.len(), dims);
            None
        };
    }

    eprintln!("embedding rate limited after 3 retries, giving up");
    None
}

/// call the self-hosted LLM API (with 429 retry)
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

    for attempt in 0..3u32 {
        let response = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
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
                eprintln!("LLM request timeout (120s)");
                return None;
            }
        };

        if response.status().as_u16() == 429 {
            let wait = if attempt < 2 { 15 } else { 30 };
            eprintln!("LLM rate limited (429), retrying in {wait}s (attempt {})", attempt + 1);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            continue;
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            eprintln!("LLM API error {status}: {}", &text[..text.len().min(200)]);
            return None;
        }

        let json: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("LLM response parse error: {e}");
                return None;
            }
        };

        return json["content"].as_str().map(|s| s.to_string());
    }

    eprintln!("LLM rate limited after 3 retries, giving up");
    None
}

/// analyze an email using self-hosted LLM for classification, summarization, and entity extraction
pub async fn analyze_email(
    config: &LlmConfig,
    sender: &str,
    subject: &str,
    body_text: &str,
) -> Option<EmailAnalysis> {
    let body_text = truncate_str(body_text, 8000);

    let system = r#"你是邮件分析助手。分析邮件并只返回一个JSON对象，不要markdown代码块，不要解释。

## 输出语言规则
summary、risk_reason、action_items、context 等文本字段一律用中文输出。category、sender_intent 等枚举值用英文。

## 分类定义 (category)
- personal: 来自朋友/家人/熟人的私人邮件
- work: 商务沟通、内部通知、项目讨论、会议邀请
- notification: 系统通知、账户提醒、密码重置、登录警告
- promotion: 营销邮件、促销、优惠券、セール、キャンペーン、広告
- newsletter: 定期资讯、博客更新、内容推荐、メルマガ
- receipt: 购买确认、发票、订单摘要
- shipping: 物流追踪、发货通知
- travel: 机票/酒店/租车确认、行程单
- finance: 银行对账、投资提醒、付款通知
- spam: 未经请求的批量邮件、垃圾广告
- scam: 钓鱼、社会工程、凭据窃取、预付费欺诈
- general: 不属于以上任何类别

## 风险评分 (risk_score)
- 0-10: 可信 — 已验证发件人，预期内容
- 11-25: 正常 — 合法营销/通知，可能有追踪像素
- 26-50: 可疑 — 未知发件人，异常请求
- 51-75: 危险 — 要求密码/银行卡/个人信息，紧迫感策略
- 76-100: 钓鱼/诈骗 — 冒充身份、假登录页面、恶意链接

## 行为检测
- requires_action: 收件人是否需要做某事（回复、审批、付款、签署、参加等）
- sender_intent: 发件人主要意图: "request", "inform", "confirm", "social", "alert"
- action_deadline: 如有截止日期，提取为 ISO 8601 (YYYY-MM-DD)，无则 null

## clean_text
提取邮件正文主要内容，去除HTML标签、导航、页眉页脚、退订链接、追踪元素。最多2000字符。

## JSON schema
{"category": "<枚举值>", "risk_score": <0-100>, "risk_reason": "<中文简要原因>", "summary": "<中文2-3句摘要>", "clean_text": "<提取的正文，最多2000字>", "requires_action": <true|false>, "sender_intent": "<request|inform|confirm|social|alert>", "action_deadline": "<ISO 8601 or null>", "people": [{"name": "...", "email": "...", "role": "..."}], "dates": [{"text": "原文", "iso_date": "YYYY-MM-DD", "context": "中文说明"}], "amounts": [{"text": "原文", "value": 123.45, "currency": "USD", "context": "中文说明"}], "action_items": ["<中文：收件人需要做的事>"]}"#;

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
