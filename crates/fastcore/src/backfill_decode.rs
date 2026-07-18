//! `POST /v1/admin/backfill-decode-headers` — one-shot repair for the
//! RFC 2047 fallout (2026-07-18 backlog #1/#2/#3).
//!
//! Messages ingested before v2.9.18 wrote raw `=?…?=` encoded-words
//! into three persistent sinks:
//!   1. thread rows (`senders_csv` / `subject` / `latest_preview`)
//!   2. the Meilisearch index (fed from those rows)
//!   3. the contacts hashes on network kevy
//!
//! This walks every user's activity zset in-process (NEVER as a
//! side-car binary — a `docker exec` second embedded-kevy open replays
//! the AOF next to the live store and OOMs the container, see the
//! 2026-07 junk-backfill incident), decodes the stored fields, writes
//! the row back when it changed, re-indexes Meili, re-derives contacts,
//! and finally scrubs encoded runes still sitting in the contact
//! hashes. Idempotent: decoded input decodes to itself.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;

use crate::FastcoreState;

pub(crate) async fn backfill_decode_headers_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut rows_decoded = 0u64;
    let mut reindexed = 0u64;
    for user in &users {
        let activity = mailrs_mailbox_kevy::keys::user_threads_by_activity(user);
        let tids = store
            .zrevrange(activity.as_bytes(), 0, -1)
            .unwrap_or_default();
        for (tid_b, _) in &tids {
            let Ok(tid) = std::str::from_utf8(tid_b) else {
                continue;
            };
            let Ok(Some(mut row)) = state.mailbox.get_thread(tid) else {
                continue;
            };
            let senders = mailrs_rfc2047::decode(row.senders_csv.as_bytes()).into_owned();
            let subject = mailrs_rfc2047::decode(row.subject.as_bytes()).into_owned();
            let preview = mailrs_rfc2047::decode(row.latest_preview.as_bytes()).into_owned();
            let dirty = senders != row.senders_csv
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
                rows_decoded += 1;
                crate::live_sync::upsert_contacts(user, &row.senders_csv);
            }
            // Re-index unconditionally — the Meili document may hold
            // encoded runes even where the row is already clean (older
            // index generations). index_meili decodes defensively too.
            crate::live_sync::index_meili(
                user,
                &row.thread_id,
                &row.subject,
                &row.senders_csv,
                &row.latest_preview,
                row.latest_date,
            );
            reindexed += 1;
            // Meili's task queue chokes on a tight firehose — same
            // pacing as the old backfill-meili binary.
            if reindexed.is_multiple_of(500) {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
    let contacts_repaired = scrub_contact_hashes(&users);
    tracing::info!(
        rows_decoded,
        reindexed,
        contacts_repaired,
        "backfill-decode-headers complete"
    );
    Json(serde_json::json!({
        "rows_decoded": rows_decoded,
        "meili_reindexed": reindexed,
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
    let Ok(mut conn) = kevy_client::Connection::open(&url) else {
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
