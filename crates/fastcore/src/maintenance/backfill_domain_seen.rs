//! Teach the domain counter what already arrived.
//!
//! # Why this is not optional
//!
//! The brand-impersonation rule is a **pair**: a display name claiming
//! a company, *and* a sending domain this deployment has no history
//! with. The corpus is unambiguous about needing both — over 35,962
//! messages the claim alone matches 32 senders and twelve of them are
//! real mail somebody wanted, while the pair matches 20 and every one
//! is a phish.
//!
//! The counter was added on 2026-08-30 and counts forward from that
//! moment. So on the day it shipped, **every domain in the world had a
//! history of zero** — including `linkedin.com` and `ameba.jp` — and
//! the rule quietly degraded into the naive version its own
//! documentation says must never ship. It held eleven legitimate
//! conversations before anybody looked: an Amazon recruiter's InMail
//! and an Ameba newsletter naming SMBC, exactly the two the
//! measurement had named as the ones to protect.
//!
//! A counter that starts at zero is not "no data yet". It is a
//! confident wrong answer, and the rule cannot tell the difference.
//!
//! # What it counts
//!
//! One increment per delivered message, per sender's registrable
//! domain — the same thing the live path counts, so the two agree
//! rather than one of them being a repair of the other.
//!
//! Bounded and dry by default, like every sweep here since the
//! outage on 2026-08-26.

use std::collections::HashMap;

use super::prelude::*;

/// Messages between pauses.
const PAUSE_EVERY: u64 = 200;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without counting. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: u64,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn yes() -> bool {
    true
}
fn default_limit() -> u64 {
    5000
}
fn default_pause_ms() -> u64 {
    20
}

/// `POST /v1/admin/maintenance:backfill-domain-seen?dry_run=false`
pub(crate) async fn backfill_domain_seen_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::error!("backfill-domain-seen: no network kevy — nothing to count into");
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let users = state.mailbox.list_account_addresses().unwrap_or_default();

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut counted = 0u64;
    let mut no_sender = 0u64;
    let mut stopped_early = false;
    // What the count would be afterwards, for the handful of domains
    // the rule was wrong about. A dry run that reported only a total
    // could not answer the question anybody actually has.
    let mut domains: HashMap<String, u64> = HashMap::new();

    'walk: for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                seen += 1;
                if seen <= q.skip {
                    continue;
                }
                if walked >= q.limit {
                    stopped_early = true;
                    break 'walk;
                }
                walked += 1;
                if walked.is_multiple_of(PAUSE_EVERY) {
                    tokio::time::sleep(std::time::Duration::from_millis(q.pause_ms)).await;
                }
                let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
                    no_sender += 1;
                    continue;
                };
                let Ok(wire) =
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
                else {
                    no_sender += 1;
                    continue;
                };
                let Some(at) = wire.sender.rfind('@') else {
                    no_sender += 1;
                    continue;
                };
                let host = wire.sender[at + 1..].trim_end_matches('>').trim();
                let d = mailrs_fraud::brand::registrable(host);
                if d.is_empty() {
                    no_sender += 1;
                    continue;
                }
                *domains.entry(d.clone()).or_default() += 1;
                counted += 1;
                if !q.dry_run {
                    mailrs_core_sidestate::families::domain_history::record_seen(&mut conn, host);
                }
            }
        }
    }

    // The ones the rule is about: familiar enough that a brand claim
    // from them is ordinary mail.
    let mut top: Vec<(&String, &u64)> = domains.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    let top: Vec<serde_json::Value> = top
        .into_iter()
        .take(20)
        .map(|(d, n)| serde_json::json!({ "domain": d, "messages": n }))
        .collect();

    tracing::info!(
        walked,
        counted,
        no_sender,
        distinct_domains = domains.len(),
        dry_run = q.dry_run,
        "backfill-domain-seen complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "messages_walked": walked,
        "counted": counted,
        "no_sender": no_sender,
        "distinct_domains": domains.len(),
        "busiest": top,
    }))
    .into_response()
}
