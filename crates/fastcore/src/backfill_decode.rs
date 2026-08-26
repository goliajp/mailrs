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
    let mut rows_decoded = 0u64;
    let mut blobs_added = 0u64;
    let mut bodies_indexed = 0u64;
    let mut trust_stamped = 0u64;
    let mut previews_filled = 0u64;
    // What it walked, not only what it changed. `previews_filled: 0`
    // alone cannot tell "every row already had one" from "there were
    // no rows" — and a number that cannot come out zero for a good
    // reason is not a verification.
    let mut threads_walked = 0u64;
    // Rows that already carried a preview. Without it, `previews_filled:
    // 0` cannot distinguish "they all had one" from "the fill never
    // fires", and 31,763 bodies were walked for a zero nobody could
    // read.
    let mut previews_present = 0u64;
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
            if threads_walked >= args.limit {
                stopped_early = true;
                break 'walk;
            }
            threads_walked += 1;
            if threads_walked.is_multiple_of(PAUSE_EVERY) {
                // Deliberately standing aside: the point is to let
                // readers have the store lock, which yielding to the
                // scheduler alone does not do.
                tokio::time::sleep(Duration::from_millis(args.pause_ms)).await;
            }
            let tid = tid.as_str();
            let Ok(Some(mut row)) = state.mailbox.get_thread(tid) else {
                continue;
            };
            let mut newest: Option<(i64, String)> = None;
            let senders = mailrs_rfc2047::decode(row.senders_csv.as_bytes()).into_owned();
            let subject = mailrs_rfc2047::decode(row.subject.as_bytes()).into_owned();
            // Re-run through `preview_line`, not only decoded: rows
            // written before it learned about rule lines carry the bar
            // a plain-text mail draws across the page —
            // `Hello HAO, ------------------------------ …` opened
            // nearly every row on a phone. Re-running over the stored
            // string is enough, because the dashes are still in it as
            // dashes; the line only gets shorter, never wrong.
            let preview = mailrs_clean::preview_line(
                &mailrs_rfc2047::decode(row.latest_preview.as_bytes()),
                120,
            );
            // Rows written before the text index existed carry no
            // `search_blob`, so they are invisible to search until
            // something rewrites them. upsert_thread synthesises the
            // field, so re-writing such a row is all it takes.
            let needs_blob = !state
                .mailbox
                .store_ref()
                .hexists(
                    mailrs_mailbox_kevy::keys::thread(tid).as_bytes(),
                    mailrs_mailbox_kevy::keys::THREAD_SEARCH_FIELD,
                )
                .unwrap_or(false);
            let dirty = needs_blob
                || senders != row.senders_csv
                || subject != row.subject
                || preview != row.latest_preview;
            if dirty {
                row.senders_csv = senders;
                row.subject = subject;
                row.latest_preview = preview;
                if let Err(e) = state.mailbox.upsert_thread(user, &row) {
                    tracing::warn!(err = %e, %user, %tid, "decode backfill: upsert failed");
                    continue;
                }
                if needs_blob {
                    blobs_added += 1;
                }
                rows_decoded += 1;
                crate::live_sync::upsert_contacts(user, &row.senders_csv);
            }
            // Body text for the message-level search index. Reads each
            // message's maildir file once — the heaviest part of this
            // sweep, and the reason it is an explicit admin action
            // rather than something the ingest path retrofits.
            for blob in state
                .mailbox
                .thread_messages_for_maintenance(tid)
                .unwrap_or_default()
            {
                let Ok(mut w) =
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&blob)
                else {
                    continue;
                };
                if w.message_id.is_empty() {
                    continue;
                }
                // This user's own filename. The shared blob's names
                // whichever owner wrote last, and reading through it looks
                // in the wrong maildir for anyone else on the thread.
                let blob_ref = state
                    .mailbox
                    .user_message_facts(user, &w.message_id)
                    .ok()
                    .flatten()
                    .map(|f| f.blob_ref)
                    .unwrap_or_else(|| w.blob_ref.clone());
                let Some(raw) = crate::read_maildir_file(user, &blob_ref) else {
                    continue;
                };
                // Stamp the sender-auth verdict on rows ingested before
                // the field existed, so browsing old mail shows the same
                // badge new mail does. Rewrite the wire only when it
                // actually gains a verdict.
                if w.sender_trust.is_empty() {
                    let trust = crate::extract_sender_trust(&raw);
                    if !trust.is_empty() {
                        w.sender_trust = trust;
                        if let Ok(payload) = serde_json::to_vec(&w) {
                            let _ = state.mailbox.upsert_user_message(
                                user,
                                tid,
                                &w.message_id,
                                w.internal_date,
                                &payload,
                                &mailrs_mailbox_kevy::UserMessageFacts {
                                    blob_ref: &w.blob_ref,
                                    uid: w.uid,
                                    flags: w.flags,
                                    modseq: w.modseq,
                                },
                            );
                            trust_stamped += 1;
                        }
                    }
                }
                let text = crate::body_text_for_search(&raw);
                if let Some(text) = text.as_deref()
                    && state
                        .mailbox
                        .index_message_text(&w.message_id, tid, text)
                        .is_ok()
                {
                    bodies_indexed += 1;
                }
                // The newest message's opening line is the row's
                // preview. Tracked by date rather than by position:
                // `thread_messages_for_maintenance` promises no order,
                // and the last one read is not the last one sent.
                if let Some(text) = text.as_deref()
                    && newest.as_ref().is_none_or(|(d, _)| w.internal_date >= *d)
                {
                    newest = Some((w.internal_date, mailrs_clean::preview_line(text, 120)));
                }
            }
            // Only when there is nothing there. Every received thread
            // written before 2026-08-09 has an empty one — the drain
            // passed "" — and a thread that already has a line does not
            // need this sweep's opinion of it.
            if !row.latest_preview.is_empty() {
                previews_present += 1;
            }
            if row.latest_preview.is_empty()
                && let Some((_, preview)) = newest
                && !preview.is_empty()
            {
                row.latest_preview = preview;
                if state.mailbox.upsert_thread(user, &row).is_ok() {
                    previews_filled += 1;
                }
            }
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
        threads_walked,
        previews_present,
        rows_decoded,
        blobs_added,
        bodies_indexed,
        previews_filled,
        trust_stamped,
        contacts_repaired,
        "backfill-decode-headers complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": args.skip + threads_walked,
        "threads_walked": threads_walked,
        "previews_present": previews_present,
        "rows_decoded": rows_decoded,
        "search_blobs_added": blobs_added,
        "bodies_indexed": bodies_indexed,
        "previews_filled": previews_filled,
        "trust_stamped": trust_stamped,
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
        let key = format!("mailrs:user:{user}:contacts");
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
