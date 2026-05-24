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
#[path = "content_scan_tests.rs"]
mod tests;
