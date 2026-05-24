//! AI / LLM-based spam-scoring stage. Gray-zone gated.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

/// Stage that calls `mailrs_intelligence::spam::classify` on messages whose
/// rule-engine + PTR score sits in the "gray zone" (`1.0 < rule_total <
/// spam_threshold`). Writes `ctx.ai_score` on success; always continues.
///
/// **Ordering requirement:** must run after `ContentScanStage` and
/// `PtrStage`, since the gating condition reads `ctx.content_score` +
/// `ctx.ptr_score`.
pub struct AiScoringStage {
    provider: Arc<dyn mailrs_intelligence::provider::LlmProvider>,
    valkey: Option<redis::aio::ConnectionManager>,
    spam_threshold: f64,
}

impl AiScoringStage {
    /// Construct an `AiScoringStage`. The `spam_threshold` is the same
    /// value used by the `Pipeline` and `make_delivery_decision`; pass them
    /// from the same config source at startup to keep them in sync.
    pub fn new(
        provider: Arc<dyn mailrs_intelligence::provider::LlmProvider>,
        valkey: Option<redis::aio::ConnectionManager>,
        spam_threshold: f64,
    ) -> Self {
        Self {
            provider,
            valkey,
            spam_threshold,
        }
    }
}

#[async_trait]
impl Stage for AiScoringStage {
    fn name(&self) -> &str {
        "ai_scoring"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let rule_total = ctx.content_score + ctx.ptr_score;
        if rule_total <= 1.0 || rule_total >= self.spam_threshold {
            return StageOutcome::Continue;
        }

        let subject = extract_header(&ctx.message, "Subject").unwrap_or_default();
        let body_preview = extract_body_preview(&ctx.message, 500);
        let cache = self
            .valkey
            .clone()
            .map(mailrs_intelligence::spam::RedisSpamCache::new);
        let cache_ref: Option<&dyn mailrs_intelligence::spam::SpamCache> = cache
            .as_ref()
            .map(|c| c as &dyn mailrs_intelligence::spam::SpamCache);
        ctx.ai_score = match mailrs_intelligence::spam::classify(
            self.provider.as_ref(),
            cache_ref,
            &ctx.sender,
            &subject,
            &body_preview,
        )
        .await
        {
            Some(result) => result.score,
            None => 0.0,
        };
        StageOutcome::Continue
    }
}

fn extract_header(message: &[u8], name: &str) -> Option<String> {
    let msg = std::str::from_utf8(message).ok()?;
    let prefix = format!("{name}: ");
    for line in msg.lines() {
        if line
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
        {
            return Some(line[prefix.len()..].trim().to_string());
        }
    }
    None
}

fn extract_body_preview(message: &[u8], max_len: usize) -> String {
    let msg = String::from_utf8_lossy(message);
    let body = msg
        .find("\r\n\r\n")
        .map(|i| &msg[i + 4..])
        .or_else(|| msg.find("\n\n").map(|i| &msg[i + 2..]))
        .unwrap_or("");
    body.chars().take(max_len).collect()
}

#[cfg(test)]
#[path = "ai_scoring_tests.rs"]
mod tests;
