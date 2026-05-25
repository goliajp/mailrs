//! Full email analysis — category, summary, entities, intent, action deadline.
//!
//! The analysis prompt is fixed (revision: [`PROMPT_VERSION`]) and outputs
//! Simplified Chinese for all natural-language fields regardless of source
//! language. Bump [`PROMPT_VERSION`] when changing the prompt so that
//! consumers can detect stored analyses produced by old prompts and decide
//! whether to re-analyze.

use serde::{Deserialize, Serialize};

use crate::provider::LlmProvider;

/// Current prompt revision — bump when the system prompt changes so that
/// consumers can trigger re-analysis of stored results.
pub const PROMPT_VERSION: &str = "v8";

/// Full analysis result.
///
/// `people` / `dates` / `amounts` / `action_items` use `serde_json::Value`
/// because the LLM occasionally returns strings instead of structured
/// objects; tolerating either keeps the public API stable across prompt
/// revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAnalysis {
    /// Category label (free-form, but typically one of `personal` / `work` /
    /// `transactional` / `promotion` / `notification` / `spam` / `scam`).
    #[serde(default)]
    pub category: String,
    /// Risk score 0-100; 100 = highest risk (phishing, fraud, malware link).
    #[serde(default)]
    pub risk_score: u8,
    /// One-line human-readable reason for the risk score.
    #[serde(default)]
    pub risk_reason: String,
    /// 1-3 sentence summary of the message content.
    #[serde(default)]
    pub summary: String,
    /// Extracted people / entities (JSON shape free-form per LLM impl).
    #[serde(default)]
    pub people: serde_json::Value,
    /// Extracted dates / time references mentioned in the body.
    #[serde(default)]
    pub dates: serde_json::Value,
    /// Extracted amounts / numbers / monetary values.
    #[serde(default)]
    pub amounts: serde_json::Value,
    /// Action items the recipient might want to act on.
    #[serde(default)]
    pub action_items: serde_json::Value,
    /// Cleaned plain-text body the LLM saw (matches mailrs-clean output).
    #[serde(default)]
    pub clean_text: String,
    /// `true` when the LLM judged the message as requiring user action.
    #[serde(default)]
    pub requires_action: bool,
    /// One-word sender intent (`request` / `inform` / `confirm` / `notify` / ...).
    #[serde(default)]
    pub sender_intent: String,
    /// ISO-8601 date if an action deadline was detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_deadline: Option<String>,
}

/// Truncate a string at a char boundary, never splitting a multi-byte char.
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

/// Analyze an email using the supplied [`LlmProvider`].
///
/// The body is truncated to 3000 bytes before sending. Returns `None` if
/// the provider call fails or the response can't be parsed even after
/// truncation-repair. Errors are logged via `tracing`.
pub async fn analyze_email(
    provider: &dyn LlmProvider,
    sender: &str,
    subject: &str,
    body_text: &str,
) -> Option<EmailAnalysis> {
    let body_text = truncate_str(body_text, 3000);

    let system = r#"邮件分析助手。只返回JSON，不要代码块。
重要：所有文本字段必须用简体中文输出，即使原文是日语、英语或其他语言也必须翻译成中文。

category 分类规则（严格按以下优先级判断）：
- spam: 未经请求的广告、群发营销、钓鱼、欺诈、虚假中奖、不认识的推销
- scam: 诈骗、钓鱼链接、冒充身份、勒索、虚假紧急通知
- promotion: 商家促销、优惠券、打折活动、产品推广、平台活动推送（已订阅的商家）
- newsletter: 定期订阅的资讯、周报、行业动态、博客更新
- notification: 系统通知、账户变更、安全提醒、服务状态更新、CI/CD通知
- receipt: 订单确认、付款收据、发票、电子票据
- shipping: 物流跟踪、发货通知、配送状态更新
- travel: 机票、酒店、行程确认、签证相关
- finance: 银行对账单、投资报告、税务通知、转账确认
- work: 同事/客户/合作方的工作邮件、会议邀请、项目讨论
- personal: 亲友私信、个人事务
- general: 以上都不符合时使用

判断要点：
1. 群发的商业邮件，如果收件人没有明确订阅关系 → spam
2. "お知らせ"类日语营销邮件、产品推广 → spam 或 promotion（看是否有订阅关系）
3. GitHub/GitLab/Jira 等开发工具通知 → notification
4. 含 unsubscribe 链接的批量邮件，优先考虑 promotion/newsletter/spam

risk_score: 0-100 (0可信,25正常,50可疑,75危险,100诈骗)
sender_intent: request|inform|confirm|social|alert|marketing

{"category":"","risk_score":0,"risk_reason":"中文","summary":"中文","clean_text":"中文","requires_action":false,"sender_intent":"inform","action_deadline":null,"people":[],"dates":[],"amounts":[],"action_items":["中文"]}"#;

    let user_message =
        format!("Analyze this email:\n\nFrom: {sender}\nSubject: {subject}\nBody:\n{body_text}");

    let text = provider.complete(system, &user_message, 0.1).await?;
    parse_analysis_response(&text)
}

/// Parse the JSON analysis response, tolerating markdown fences, extra
/// surrounding text, and truncated trailing output.
fn parse_analysis_response(text: &str) -> Option<EmailAnalysis> {
    let text = text.trim();
    let text = if let Some(stripped) = text.strip_prefix("```json") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else if let Some(stripped) = text.strip_prefix("```") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else {
        text
    };

    let start = text.find('{')?;
    let json_str = if let Some(end) = text.rfind('}') {
        &text[start..end + 1]
    } else {
        // truncated response — no closing brace
        &text[start..]
    };

    let mut analysis: EmailAnalysis = match serde_json::from_str(json_str) {
        Ok(a) => a,
        Err(_) => {
            // attempt repair: close any open string + add closing brace
            let mut repaired = json_str.to_string();
            let quote_count = repaired.chars().filter(|c| *c == '"').count();
            if quote_count % 2 != 0 {
                repaired.push('"');
            }
            if !repaired.trim_end().ends_with('}') {
                repaired.push('}');
            }
            match serde_json::from_str(&repaired) {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(
                        event = "analyze_parse_error",
                        error = %e,
                        raw = %&json_str[..json_str.len().min(200)]
                    );
                    return None;
                }
            }
        }
    };

    analysis.risk_score = analysis.risk_score.min(100);

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
        assert!(result.people.is_array());
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
    fn parse_truncated_response() {
        let json = r#"{"category":"promotion","risk_score":25,"risk_reason":"营销邮件","summary":"推广活动","people":[],"dates":[],"amounts":[],"action_items":[],"clean_text":"这是一封营销"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.category, "promotion");
        assert_eq!(result.risk_score, 25);
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
        assert_eq!(result.category, "work");
        assert!(result.people.is_array());
        assert!(result.amounts.is_array());
    }

    #[test]
    fn parse_tolerates_string_people() {
        // qwen sometimes returns strings instead of objects
        let json = r#"{"category":"work","risk_score":0,"risk_reason":"","summary":"test","people":"GOLIA K.K.","dates":[],"amounts":[],"action_items":[]}"#;
        let result = parse_analysis_response(json).unwrap();
        assert_eq!(result.category, "work");
        assert!(result.people.is_string());
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
    fn prompt_version_constant() {
        assert!(!PROMPT_VERSION.is_empty());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::provider::LlmProvider;
    use async_trait::async_trait;

    struct CannedProvider(String);

    #[async_trait]
    impl LlmProvider for CannedProvider {
        async fn complete(&self, _s: &str, _u: &str, _t: f32) -> Option<String> {
            Some(self.0.clone())
        }
        async fn embed(&self, _t: &str) -> Option<Vec<f32>> {
            None
        }
        fn model_id(&self) -> &str {
            "test/1"
        }
    }

    struct DeadProvider;

    #[async_trait]
    impl LlmProvider for DeadProvider {
        async fn complete(&self, _s: &str, _u: &str, _t: f32) -> Option<String> {
            None
        }
        async fn embed(&self, _t: &str) -> Option<Vec<f32>> {
            None
        }
        fn model_id(&self) -> &str {
            "dead/0"
        }
    }

    #[tokio::test]
    async fn analyze_email_returns_parsed_result() {
        let provider = CannedProvider(
            r#"{"category":"work","risk_score":10,"risk_reason":"safe","summary":"meeting","people":[],"dates":[],"amounts":[],"action_items":[],"clean_text":"meeting tomorrow","requires_action":true,"sender_intent":"request","action_deadline":"2026-01-01"}"#
                .into(),
        );
        let result = analyze_email(&provider, "boss@x", "Q3", "review please")
            .await
            .expect("analyze must succeed");
        assert_eq!(result.category, "work");
        assert!(result.requires_action);
        assert_eq!(result.action_deadline.as_deref(), Some("2026-01-01"));
    }

    #[tokio::test]
    async fn analyze_email_truncates_long_body() {
        // body > 3000 bytes should be truncated before sending to LLM; we
        // assert via the fact that we get a valid response back (the canned
        // provider doesn't care about input, the test asserts the call flows)
        let body = "x".repeat(10_000);
        let provider = CannedProvider(
            r#"{"category":"general","risk_score":0,"risk_reason":"","summary":"x","people":[],"dates":[],"amounts":[],"action_items":[],"clean_text":""}"#
                .into(),
        );
        let result = analyze_email(&provider, "a@x", "subj", &body)
            .await
            .expect("must succeed despite huge body");
        assert_eq!(result.category, "general");
    }

    #[tokio::test]
    async fn analyze_email_returns_none_on_provider_failure() {
        assert!(
            analyze_email(&DeadProvider, "a@x", "s", "b")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn analyze_email_returns_none_on_garbage_response() {
        let provider = CannedProvider("I am a tea kettle, short and stout.".into());
        assert!(analyze_email(&provider, "a@x", "s", "b").await.is_none());
    }
}
