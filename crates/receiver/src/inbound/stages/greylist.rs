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
    /// Network kevy for the known-correspondent bypass — anyone already
    /// in the recipient's contacts (we mailed them / they mailed us
    /// before) is never greylisted. None = bypass disabled.
    contacts: Option<Arc<crate::kevy_net::KevyNetClient>>,
}

impl GreylistStage {
    /// Construct a stage from its dependencies.
    pub fn new(
        db: Arc<GreylistDb>,
        config: GreylistConfig,
        remote_whitelist: GreylistListsHandle,
        local_lists: GreylistLocalHandle,
        contacts: Option<Arc<crate::kevy_net::KevyNetClient>>,
    ) -> Self {
        Self {
            db,
            config,
            remote_whitelist,
            local_lists,
            contacts,
        }
    }

    /// Known-correspondent check: HGET the recipient user's contacts
    /// hash for the sender address. Every failure mode (kevy down,
    /// join error) reads as "not a contact" — the sender then just
    /// takes the normal triplet path, never a reject.
    async fn is_known_correspondent(&self, recipient: &str, sender: &str) -> bool {
        let Some(client) = &self.contacts else {
            return false;
        };
        if sender.is_empty() || recipient.is_empty() {
            return false;
        }
        let key = format!("mailrs:user:{}:contacts", recipient.to_lowercase());
        let field = sender.to_lowercase();
        let c = client.clone();
        tokio::task::spawn_blocking(move || {
            c.with_conn(|conn| conn.hget(key.as_bytes(), field.as_bytes()))
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .is_some()
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
        let remote_synced = {
            let lists = self.remote_whitelist.read().await;
            if let Some(domain) = ctx.sender.rsplit_once('@').map(|(_, d)| d)
                && !domain.is_empty()
                && lists.is_whitelisted(domain)
            {
                metrics::counter!("mailrs_greylist_whitelist_hit_total").increment(1);
                tracing::debug!(
                    target: "greylist",
                    sender = %ctx.sender,
                    "remote whitelist hit — skipping greylist"
                );
                return StageOutcome::Continue;
            }
            lists.last_sync_at.is_some()
        };

        // 3.5 known correspondent — anyone in the recipient's contacts
        //     (prior send OR receive) is never greylisted. The strongest
        //     deliverability guarantee for real correspondents.
        if self
            .is_known_correspondent(&ctx.recipient, &ctx.sender)
            .await
        {
            metrics::counter!("mailrs_greylist_contact_bypass_total").increment(1);
            tracing::debug!(
                target: "greylist",
                sender = %ctx.sender,
                recipient = %ctx.recipient,
                "known correspondent — skipping greylist"
            );
            return StageOutcome::Continue;
        }

        // 3.9 fail-open: if the remote whitelist has NEVER been populated
        //     (no successful sync and no disk cache) we cannot tell gmail
        //     from a drive-by. Deliverability beats spam filtering — skip
        //     greylisting entirely instead of deferring legitimate mail.
        if !remote_synced {
            metrics::counter!("mailrs_greylist_failopen_total").increment(1);
            tracing::warn!(
                target: "greylist",
                sender = %ctx.sender,
                "remote whitelist never populated — greylist failing OPEN"
            );
            return StageOutcome::Continue;
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
