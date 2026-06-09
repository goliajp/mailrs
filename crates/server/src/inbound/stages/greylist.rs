//! Greylisting stage — defers first-time triplets to suppress drive-by spam.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{DeliveryDecision, ReceiveContext, Stage, StageOutcome};
use mailrs_shield::greylist::{self as greylisting, GreylistConfig, GreylistDb, GreylistDecision};

use crate::greylist_sync::GreylistListsHandle;

/// Stage that checks the (client_ip, sender, recipient) triplet against a
/// greylist store; on `Defer`/`TooEarly`, short-circuits the pipeline with
/// [`DeliveryDecision::Greylist`]. On `Accept`, continues.
///
/// Before doing the triplet lookup, the stage consults a remote-synced
/// sender-domain whitelist ([`GreylistListsHandle`]). Hits skip greylist
/// entirely — used to let well-known providers (Gmail, Outlook, etc.)
/// through on first try since their retry behavior is unreliable.
pub struct GreylistStage {
    db: Arc<GreylistDb>,
    config: GreylistConfig,
    whitelist: GreylistListsHandle,
}

impl GreylistStage {
    /// Construct a `GreylistStage` bound to a `GreylistDb`, its config,
    /// and the shared whitelist handle from `greylist_sync`.
    pub fn new(
        db: Arc<GreylistDb>,
        config: GreylistConfig,
        whitelist: GreylistListsHandle,
    ) -> Self {
        Self {
            db,
            config,
            whitelist,
        }
    }
}

#[async_trait]
impl Stage for GreylistStage {
    fn name(&self) -> &str {
        "greylist"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        // Whitelist short-circuit. Splits the sender on '@' and ancestor-
        // walks the domain (mail.gmail.com → gmail.com etc.). Empty senders
        // (bounces, '<>') silently fall through to the triplet check.
        if let Some(domain) = ctx.sender.rsplit_once('@').map(|(_, d)| d)
            && !domain.is_empty()
        {
            let lists = self.whitelist.read().await;
            if lists.is_whitelisted(domain) {
                metrics::counter!("mailrs_greylist_whitelist_hit_total").increment(1);
                tracing::debug!(
                    target: "greylist",
                    sender = %ctx.sender,
                    "whitelist hit — skipping greylist"
                );
                return StageOutcome::Continue;
            }
        }

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
