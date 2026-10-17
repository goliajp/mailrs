//! Greylisting stage — defers first-time triplets to suppress drive-by spam.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{AllowListMatch, DeliveryDecision, ReceiveContext, Stage, StageOutcome};
use mailrs_shield::greylist::{self as greylisting, GreylistConfig, GreylistDb, GreylistDecision};

use crate::greylist_sync::GreylistListsHandle;

/// Stage that consults the whitelists and then the triplet store:
///
/// 1. local-white hit (recorded by `local_lists`) → skip greylist
/// 2. remote-white hit (Phase 1) → skip greylist
/// 3. triplet check → Defer / TooEarly / Accept
pub struct GreylistStage {
    db: Arc<GreylistDb>,
    config: GreylistConfig,
    remote_whitelist: GreylistListsHandle,
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
        contacts: Option<Arc<crate::kevy_net::KevyNetClient>>,
    ) -> Self {
        Self {
            db,
            config,
            remote_whitelist,
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
            c.with_conn(|conn| {
                conn.hget(key.as_bytes(), field.as_bytes())
                    .map_err(std::io::Error::from)
            })
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
        // 1-2. the local lists ran first (`local_lists`): a black hit never
        //      gets here, and any white hit skips greylisting, proven or not
        if ctx.allow_list != AllowListMatch::None {
            return StageOutcome::Continue;
        }

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
