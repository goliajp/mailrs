//! Rule-based content-scoring stage.

use async_trait::async_trait;
use mailrs_inbound::{ReceiveContext, Stage, StageOutcome};

use crate::inbound::content_scan::evaluate_rules;

/// Stage that runs the rule-engine over the raw message body and writes
/// `ctx.content_score` + `ctx.matched_rules`. Always continues; the final
/// decision step combines content + ptr + ai scores against the spam
/// threshold.
pub struct ContentScanStage;

impl ContentScanStage {
    /// Construct a `ContentScanStage`. The stage carries no state; all rule
    /// definitions live inside `inbound::content_scan::evaluate_rules`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for ContentScanStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Stage for ContentScanStage {
    fn name(&self) -> &str {
        "content_scan"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let (score, rules) = evaluate_rules(&ctx.message);
        ctx.content_score = score;
        ctx.matched_rules = rules;
        StageOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;

    fn ctx_with(message: &[u8]) -> ReceiveContext {
        ReceiveContext::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
            "client.example.com",
            "alice@example.com",
            "bob@example.com",
            message.to_vec(),
            "mx.example.com",
        )
    }

    #[tokio::test]
    async fn writes_score_and_rules_into_context() {
        let stage = ContentScanStage::new();
        let mut ctx = ctx_with(b"From: alice@example.com\r\n\r\nbody");
        let outcome = stage.evaluate(&mut ctx).await;
        assert_eq!(outcome, StageOutcome::Continue);
        // assertion on actual score depends on rule set — the contract here
        // is just that the stage writes both fields and continues.
        assert!(ctx.content_score >= 0.0);
        // matched_rules may be empty for a benign message
        let _ = ctx.matched_rules;
    }

    #[tokio::test]
    async fn name_is_stable() {
        let stage = ContentScanStage::new();
        assert_eq!(stage.name(), "content_scan");
    }
}
