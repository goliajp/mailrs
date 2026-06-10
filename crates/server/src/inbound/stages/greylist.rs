//! Greylisting stage — defers first-time triplets to suppress drive-by spam.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{DeliveryDecision, ReceiveContext, Stage, StageOutcome};
use mailrs_shield::greylist::{self as greylisting, GreylistConfig, GreylistDb, GreylistDecision};

use crate::greylist_local::GreylistLocalHandle;
use crate::greylist_sync::GreylistListsHandle;

/// Stage that consults local + remote whitelists and then the triplet
/// store. Pipeline order:
///
/// 1. local-black hit → 550 5.7.1 reject
/// 2. local-white hit → skip greylist
/// 3. remote-white hit (Phase 1) → skip greylist
/// 4. triplet check → Defer / TooEarly / Accept
///
/// Schema mutex `UNIQUE (kind, value)` makes step 2 unreachable when step
/// 1 fires; the black-before-white code ordering is a belt-and-suspenders.
pub struct GreylistStage {
    db: Arc<GreylistDb>,
    config: GreylistConfig,
    remote_whitelist: GreylistListsHandle,
    local_lists: GreylistLocalHandle,
}

impl GreylistStage {
    /// Construct a stage from its dependencies.
    pub fn new(
        db: Arc<GreylistDb>,
        config: GreylistConfig,
        remote_whitelist: GreylistListsHandle,
        local_lists: GreylistLocalHandle,
    ) -> Self {
        Self {
            db,
            config,
            remote_whitelist,
            local_lists,
        }
    }
}

#[async_trait]
impl Stage for GreylistStage {
    fn name(&self) -> &str {
        "greylist"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        // 1. local black — checked first so that a single black entry can
        //    punch a hole through a broad white entry (matcher contract:
        //    any black-kind hit beats any white-kind hit).
        let local = self.local_lists.read().await;
        if let Some(kind) = local.matches_black(&ctx.sender, ctx.client_ip) {
            metrics::counter!("mailrs_greylist_local_black_hit_total", "kind" => kind).increment(1);
            tracing::info!(
                target: "greylist",
                sender = %ctx.sender,
                client_ip = %ctx.client_ip,
                kind = %kind,
                "local black hit — rejecting 550"
            );
            return StageOutcome::Decide(DeliveryDecision::Reject {
                code: 550,
                message: "5.7.1 Message rejected: local policy denied".to_string(),
            });
        }

        // 2. local white — skip the rest of the greylist (no triplet,
        //    no remote whitelist needed).
        if let Some(kind) = local.matches_white(&ctx.sender, ctx.client_ip) {
            metrics::counter!("mailrs_greylist_local_white_hit_total", "kind" => kind).increment(1);
            tracing::debug!(
                target: "greylist",
                sender = %ctx.sender,
                kind = %kind,
                "local white hit — skipping greylist"
            );
            return StageOutcome::Continue;
        }
        drop(local);

        // 3. remote whitelist (Phase 1). Splits the sender on '@' and
        //    ancestor-walks the domain (mail.gmail.com → gmail.com etc.).
        //    Empty senders (bounces, '<>') silently fall through to the
        //    triplet check.
        if let Some(domain) = ctx.sender.rsplit_once('@').map(|(_, d)| d)
            && !domain.is_empty()
        {
            let lists = self.remote_whitelist.read().await;
            if lists.is_whitelisted(domain) {
                metrics::counter!("mailrs_greylist_whitelist_hit_total").increment(1);
                tracing::debug!(
                    target: "greylist",
                    sender = %ctx.sender,
                    "remote whitelist hit — skipping greylist"
                );
                return StageOutcome::Continue;
            }
        }

        // 4. triplet check.
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
