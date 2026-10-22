//! `POST /v1/admin/backfill-decode-headers` — one-shot repair for the
//! RFC 2047 fallout (2026-07-18 backlog #1/#2/#3).
//!
//! Messages ingested before v2.9.18 wrote raw `=?…?=` encoded-words
//! into three persistent sinks:
//!   1. thread rows (`senders_csv` / `subject` / `latest_preview`)
//!   2. the contacts hashes on network kevy
//!
//! This walks every user's activity zset in-process (NEVER as a
//! side-car binary — a `docker exec` second embedded-kevy open replays
//! the AOF next to the live store and OOMs the container, see the
//! 2026-07 junk-backfill incident), decodes the stored fields, writes
//! the row back when it changed, re-derives contacts, and scrubs
//! encoded runes still sitting in the contact hashes. Rewriting a row
//! also (re)builds its `search_blob`, so the kevy text index picks it
//! up for free. Idempotent: decoded input decodes to itself.
//!
//! **Bounded, and it pauses.** Run unbounded on 2026-08-26 it walked
//! thirty thousand threads for half an hour, reading a maildir file and
//! parsing HTML for each, and took the mail service down while it did:
//! the embedded store has one lock, and a sweep that reaches for it
//! without stopping starves every reader behind it. The conversation
//! list simply span. Stopping it needed a SIGKILL — the graceful path
//! could not be scheduled — which is how this repo has corrupted an AOF
//! before.
//!
//! So a call now walks at most `limit` threads, sleeps for `pause_ms`
//! every `PAUSE_EVERY` of them, and answers with where to resume.
//! Repair is a sequence of short bursts anyone can stop between, not
//! one long outage.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;

use crate::FastcoreState;

mod thread;

/// How many threads between pauses. Small enough that a reader waiting
/// on the store lock is not held for long.
const PAUSE_EVERY: u64 = 25;

#[derive(serde::Deserialize)]
pub(crate) struct SweepArgs {
    /// Threads to step over before working — where the last call said
    /// to resume.
    #[serde(default)]
    skip: u64,
    /// The most threads this call will walk.
    #[serde(default = "default_limit")]
    limit: u64,
    /// How long to stand aside every `PAUSE_EVERY` threads.
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn default_limit() -> u64 {
    500
}

fn default_pause_ms() -> u64 {
    50
}

/// What a call changed, and what it walked to get there.
#[derive(Default)]
struct Counts {
    rows_decoded: u64,
    blobs_added: u64,
    bodies_indexed: u64,
    trust_stamped: u64,
    previews_filled: u64,
    // What it walked, not only what it changed. `previews_filled: 0`
    // alone cannot tell "every row already had one" from "there were
    // no rows" — and a number that cannot come out zero for a good
    // reason is not a verification.
    threads_walked: u64,
    // Rows that already carried a preview. Without it, `previews_filled:
    // 0` cannot distinguish "they all had one" from "the fill never
    // fires", and 31,763 bodies were walked for a zero nobody could
    // read.
    previews_present: u64,
}

pub(crate) async fn backfill_decode_headers_route(
    State(state): State<Arc<FastcoreState>>,
    Query(args): Query<SweepArgs>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut c = Counts::default();
    // Position in the whole walk, counted whether or not the thread was
    // worked on — it is what `skip` is measured in.
    let mut seen = 0u64;
    let mut stopped_early = false;
    'walk: for user in &users {
        // Declared rows; `user_threads_by_activity` is legacy and unwritten.
        let tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        for tid in &tids {
            seen += 1;
            if seen <= args.skip {
                continue;
            }
            if c.threads_walked >= args.limit {
                stopped_early = true;
                break 'walk;
            }
            c.threads_walked += 1;
            if c.threads_walked.is_multiple_of(PAUSE_EVERY) {
                // Deliberately standing aside: the point is to let
                // readers have the store lock, which yielding to the
                // scheduler alone does not do.
                tokio::time::sleep(Duration::from_millis(args.pause_ms)).await;
            }
            thread::repair_thread(&state, user, tid.as_str(), &mut c);
        }
    }
    // Only on the pass that reaches the end: it walks every contact
    // hash, and doing that once per batch would be the same starvation
    // in a smaller shape.
    let contacts_repaired = if stopped_early {
        0
    } else {
        scrub_contact_hashes(&users)
    };
    tracing::info!(
        threads_walked = c.threads_walked,
        previews_present = c.previews_present,
        rows_decoded = c.rows_decoded,
        blobs_added = c.blobs_added,
        bodies_indexed = c.bodies_indexed,
        previews_filled = c.previews_filled,
        trust_stamped = c.trust_stamped,
        contacts_repaired,
        "backfill-decode-headers complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": args.skip + c.threads_walked,
        "threads_walked": c.threads_walked,
        "previews_present": c.previews_present,
        "rows_decoded": c.rows_decoded,
        "search_blobs_added": c.blobs_added,
        "bodies_indexed": c.bodies_indexed,
        "previews_filled": c.previews_filled,
        "trust_stamped": c.trust_stamped,
        "contacts_repaired": contacts_repaired,
    }))
    .into_response()
}

/// Scrub `=?…?=` runes left in the per-user contact hashes on network
/// kevy: decode poisoned display values in place; drop fields whose
/// key itself is an encoded rune (the re-derive above re-adds the
/// proper entry).
fn scrub_contact_hashes(users: &[String]) -> u64 {
    let Some(url) = crate::live_sync::network_kevy_url() else {
        return 0;
    };
    let Ok(mut conn) = kevy_client::Connection::connect(&url) else {
        return 0;
    };
    let mut repaired = 0u64;
    for user in users {
        let key = mailrs_core_sidestate::families::contact_cards::autocomplete_key(user);
        let flat = conn.hgetall(key.as_bytes()).unwrap_or_default();
        for pair in flat.chunks(2) {
            let [field, value] = pair else { continue };
            let field_str = String::from_utf8_lossy(field);
            let value_str = String::from_utf8_lossy(value);
            if field_str.contains("=?") {
                let _ = conn.hdel(key.as_bytes(), &[field]);
                repaired += 1;
                continue;
            }
            if value_str.contains("=?") {
                let decoded = mailrs_rfc2047::decode(value).into_owned();
                if decoded != value_str {
                    let _ = conn.hset(key.as_bytes(), &[(field.as_slice(), decoded.as_bytes())]);
                    repaired += 1;
                }
            }
        }
    }
    repaired
}
