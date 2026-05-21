//! Greylisting stage — defers first-time triplets to suppress drive-by spam.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{DeliveryDecision, ReceiveContext, Stage, StageOutcome};
use mailrs_shield::greylist::{self as greylisting, GreylistConfig, GreylistDb, GreylistDecision};

/// Stage that checks the (client_ip, sender, recipient) triplet against a
/// greylist store; on `Defer`/`TooEarly`, short-circuits the pipeline with
/// [`DeliveryDecision::Greylist`]. On `Accept`, continues.
pub struct GreylistStage {
    db: Arc<GreylistDb>,
    config: GreylistConfig,
}

impl GreylistStage {
    /// Construct a `GreylistStage` bound to an existing `GreylistDb` and
    /// its config.
    pub fn new(db: Arc<GreylistDb>, config: GreylistConfig) -> Self {
        Self { db, config }
    }
}

#[async_trait]
impl Stage for GreylistStage {
    fn name(&self) -> &str {
        "greylist"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        let key = greylisting::triplet_key(&ctx.client_ip.to_string(), &ctx.sender, &ctx.recipient);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        match self.db.check(&key, now, &self.config).await {
            GreylistDecision::Defer | GreylistDecision::TooEarly => {
                ctx.greylisted = true;
                StageOutcome::Decide(DeliveryDecision::Greylist)
            }
            GreylistDecision::Accept => StageOutcome::Continue,
        }
    }
}
