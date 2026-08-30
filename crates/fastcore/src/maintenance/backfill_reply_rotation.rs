//! Teach the reply-address sets what already arrived.
//!
//! # Why this one is not optional either
//!
//! [`mailrs_fraud::reply_rotation`] convicts on the **fourth**
//! registrable domain funnelling into one off-domain reply address.
//! The live path records a domain as each message arrives, so on the
//! day it shipped every reply address in the world had one domain —
//! or none — and the rule could not fire at all.
//!
//! That is the same failure as the domain counter's
//! ([`super::backfill_domain_seen`]), pointing the other way: a set
//! that starts empty makes every rotating sender look settled, and
//! the 258 messages already sitting in the corpus stay where they
//! are. A rule that only ever sees arriving mail catches a
//! campaign's tail and leaves its head in the inbox.
//!
//! **This is the reason detection has to be able to run over
//! history**, not only at the door.
//!
//! # What it records
//!
//! One `(reply address, registrable sending domain)` pair per
//! message — the same pair the live path records, through the same
//! function, so the two agree rather than one being a repair of the
//! other.
//!
//! Every message, not the newest of each conversation: the count is
//! *how many domains*, and a campaign that sends 179 messages from
//! six domains is only visible if all six are looked at.
//!
//! Bounded and dry by default.

use std::collections::HashMap;

use super::prelude::*;

/// Messages between pauses.
const PAUSE_EVERY: u64 = 200;

#[derive(serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without recording. **Default true.**
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

/// `POST /v1/admin/maintenance:backfill-reply-rotation?dry_run=false`
pub(crate) async fn backfill_reply_rotation_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    let Some(url) = crate::live_sync::network_kevy_url() else {
        tracing::error!("backfill-reply-rotation: no network kevy — nothing to record into");
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let users = state.mailbox.list_account_addresses().unwrap_or_default();

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut recorded = 0u64;
    let mut no_file = 0u64;
    let mut same_estate = 0u64;
    let mut stopped_early = false;
    // Reply address -> the sending domains that funnel into it. Held
    // here as well as written, so a **dry run answers the question**:
    // which addresses would cross the threshold, and with what. A
    // sweep that reported only a total could be read as "nothing
    // found" when it means "not recorded yet".
    let mut targets: HashMap<String, std::collections::BTreeSet<String>> = HashMap::new();

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
                let Some(raw) = super::fraud_rescan::reading::raw_for_message(&state, user, &mid)
                else {
                    no_file += 1;
                    continue;
                };
                let from = mailrs_inbound::identity::from_header(&raw);
                let Some(at) = from.rfind('@') else {
                    no_file += 1;
                    continue;
                };
                let host = from[at + 1..].trim_end_matches('>').trim().to_string();
                let reply = mailrs_inbound::identity::reply_to_address(&raw);
                if !mailrs_fraud::reply_rotation::is_off_domain(&host, &reply) {
                    same_estate += 1;
                    continue;
                }
                let domain = mailrs_fraud::brand::registrable(&host);
                targets.entry(reply.clone()).or_default().insert(domain);
                recorded += 1;
                if !q.dry_run {
                    mailrs_core_sidestate::families::reply_rotation::record(
                        &mut conn, &host, &reply,
                    );
                }
            }
        }
    }

    // The ones the rule is about, and the ones just below it — a
    // report that showed only the convicted could not say whether
    // the threshold is in the right place.
    let mut rows: Vec<(&String, &std::collections::BTreeSet<String>)> =
        targets.iter().filter(|(_, d)| d.len() >= 2).collect();
    rows.sort_by_key(|(_, d)| std::cmp::Reverse(d.len()));
    let rows: Vec<serde_json::Value> = rows
        .into_iter()
        .take(20)
        .map(|(addr, doms)| {
            serde_json::json!({
                "reply_to": addr,
                "domains": doms.len(),
                "rotates": mailrs_fraud::reply_rotation::rotates(doms.len() as u32),
                "sending_domains": doms.iter().take(8).collect::<Vec<_>>(),
            })
        })
        .collect();

    let rotating = targets
        .values()
        .filter(|d| mailrs_fraud::reply_rotation::rotates(d.len() as u32))
        .count();

    tracing::info!(
        walked,
        recorded,
        same_estate,
        no_file,
        reply_targets = targets.len(),
        rotating,
        dry_run = q.dry_run,
        "backfill-reply-rotation complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "messages_walked": walked,
        "recorded": recorded,
        "same_estate": same_estate,
        "no_file": no_file,
        "reply_targets": targets.len(),
        "rotating": rotating,
        "threshold": mailrs_fraud::reply_rotation::ROTATION_THRESHOLD,
        "busiest": rows,
    }))
    .into_response()
}
