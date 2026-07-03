//! Quota stage — tempfail (452 4.2.2) recipients whose mailbox is over
//! its byte quota.
//!
//! Data lives in the shared network kevy so the receiver can consult it
//! without touching fastcore's embedded store:
//!
//!   mailrs:quota:<user>:limit_bytes   set by fastcore's set_quota RPC
//!   mailrs:quota:<user>:used_bytes    maintained by the delivery paths
//!                                     (spool drain, mirror send, IMAP
//!                                     APPEND) and the backfill-usage tool
//!
//! Enforcement happens HERE and only here: once the receiver answers
//! 250 the message is accepted and must never be dropped (deliverability
//! rule) — the drain always delivers and only accounts. 452 is a
//! tempfail, so a compliant sender retries after the user frees space.
//!
//! Fail-open everywhere: kevy unreachable, missing keys, parse garbage
//! → no quota enforcement. Missing/zero limit = unlimited.

use std::sync::Arc;

use async_trait::async_trait;
use mailrs_inbound::{DeliveryDecision, ReceiveContext, Stage, StageOutcome};

use crate::kevy_net::KevyNetClient;

/// Key helpers — shared shape with the fastcore-side writers.
pub fn limit_key(user: &str) -> String {
    format!("mailrs:quota:{}:limit_bytes", user.to_lowercase())
}

/// See [`limit_key`].
pub fn used_key(user: &str) -> String {
    format!("mailrs:quota:{}:used_bytes", user.to_lowercase())
}

/// Stage implementation. `None` client (monolith lane) disables the
/// stage entirely.
pub struct QuotaStage {
    client: Option<Arc<KevyNetClient>>,
}

impl QuotaStage {
    /// Construct the stage.
    pub fn new(client: Option<Arc<KevyNetClient>>) -> Self {
        Self { client }
    }

    async fn over_quota(&self, recipient: &str) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let lk = limit_key(recipient);
        let uk = used_key(recipient);
        let c = client.clone();
        let pair = tokio::task::spawn_blocking(move || {
            c.with_conn(|conn| {
                let limit = conn.get(lk.as_bytes())?;
                let used = conn.get(uk.as_bytes())?;
                Ok((limit, used))
            })
        })
        .await
        .ok()
        .and_then(Result::ok);
        let Some((limit, used)) = pair else {
            return false; // kevy down — fail open
        };
        let parse = |v: Option<Vec<u8>>| {
            v.and_then(|b| String::from_utf8(b).ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
        };
        let limit = parse(limit);
        let used = parse(used);
        limit > 0 && used >= limit
    }
}

#[async_trait]
impl Stage for QuotaStage {
    fn name(&self) -> &str {
        "quota"
    }

    async fn evaluate(&self, ctx: &mut ReceiveContext) -> StageOutcome {
        if self.over_quota(&ctx.recipient).await {
            metrics::counter!("mailrs_quota_tempfail_total").increment(1);
            tracing::info!(
                target: "quota",
                recipient = %ctx.recipient,
                "recipient over quota — 452 tempfail"
            );
            return StageOutcome::Decide(DeliveryDecision::Reject {
                code: 452,
                message: "4.2.2 Mailbox over quota, try again later".to_string(),
            });
        }
        StageOutcome::Continue
    }
}
