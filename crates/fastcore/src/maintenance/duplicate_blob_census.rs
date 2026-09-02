//! One file on disk, two rows in the store.
//!
//! A message delivered by the monolith's `process_delivered` was
//! registered under its real `Message-ID`; something later registered
//! the same maildir file again under the synthetic id that path mints
//! when a message has none — `{now}.{maildir_id}@mailrs.local`.
//!
//! Measured on production 2026-09-02:
//!
//! ```text
//! blob_ref 1781060503.M358220P1Q61.41b39a97e35f
//!   file on disk                                             1
//!   SA1PR17MB5447…@outlook.com        uid 12168   thread A
//!   1781060503.…@mailrs.local         uid 22608   thread B
//! ```
//!
//! Two UIDs and two conversations for one message, so it appears
//! twice in every list and twice in an IMAP client. 111 conversations
//! of 34,491 carry a synthetic id.
//!
//! # The writer is dead, which is why this counts rather than repairs
//!
//! The synthetic id is minted only by
//! `crates/server/src/smtp_session/process_delivered.rs`, in
//! `mailrs-server` — out of the image since
//! `.claude/rfcs/20260722-monolith-out-of-image.md`. Their dates run
//! 2026-03-04 to **2026-07-02** and stop there, twenty days before
//! that removal and two months before today.
//!
//! So this is a stock of old damage, not a live writer, and the first
//! thing to establish is its true shape: **`@mailrs.local` is a
//! symptom, not the definition.** A duplicate registration under two
//! real ids would look identical to a reader and would not carry that
//! suffix. This walks every message and groups by `blob_ref`, so the
//! number it reports is the defect rather than one visible marker of
//! it.
//!
//! Read-only. Deleting a row is a separate decision and needs to know
//! which of the two a reader has already interacted with — the UIDs
//! differ, so an IMAP client has both, and removing the wrong one
//! breaks a client's UID map.

use std::collections::HashMap;

use super::prelude::*;

/// Messages between pauses.
const PAUSE_EVERY: u64 = 400;

#[derive(serde::Deserialize)]
pub(crate) struct CensusQuery {
    #[serde(default)]
    skip: u64,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
}

fn default_limit() -> u64 {
    200_000
}
fn default_pause_ms() -> u64 {
    20
}

/// One `blob_ref` and the message rows that claim it.
struct Claim {
    user: String,
    message_id: String,
    thread_id: String,
    uid: u32,
    date: i64,
}

/// `POST /v1/admin/maintenance:duplicate-blob-census`
pub(crate) async fn duplicate_blob_census_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<CensusQuery>,
) -> axum::response::Response {
    let users = state.mailbox.list_account_addresses().unwrap_or_default();

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut no_blob = 0u64;
    let mut stopped_early = false;
    // blob_ref -> every row that names it. Keyed per user as well,
    // because the same file legitimately backs one row per recipient
    // and that is not a duplicate.
    let mut claims: HashMap<(String, String), Vec<Claim>> = HashMap::new();

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
                    continue;
                };
                let Ok(wire) =
                    serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
                else {
                    continue;
                };
                if wire.blob_ref.is_empty() {
                    no_blob += 1;
                    continue;
                }
                claims
                    .entry((user.clone(), wire.blob_ref.clone()))
                    .or_default()
                    .push(Claim {
                        user: user.clone(),
                        message_id: wire.message_id.clone(),
                        thread_id: wire.thread_id.clone(),
                        uid: wire.uid,
                        date: wire.date,
                    });
            }
        }
    }

    let dupes: Vec<_> = claims.iter().filter(|(_, v)| v.len() > 1).collect();
    // Split by whether a synthetic id is involved, because that says
    // whether the known-dead writer explains the whole stock or only
    // part of it. A pair of real ids would mean a second writer.
    let mut with_synthetic = 0u64;
    let mut all_real = 0u64;
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut real_samples: Vec<serde_json::Value> = Vec::new();
    for ((_, blob), rows) in &dupes {
        let synthetic = rows.iter().any(|c| c.message_id.ends_with("@mailrs.local"));
        if synthetic {
            with_synthetic += 1;
        } else {
            all_real += 1;
        }
        let sample = serde_json::json!({
            "blob_ref": blob,
            "rows": rows.iter().map(|c| serde_json::json!({
                "user": c.user,
                "message_id": c.message_id,
                "thread_id": c.thread_id,
                "uid": c.uid,
                "date": c.date,
            })).collect::<Vec<_>>(),
        });
        match synthetic {
            true if samples.len() < 10 => samples.push(sample),
            // Kept separate and never truncated below ten: a pair of
            // real ids is the case this census exists to find, and
            // reporting it only as a count would be a number nobody
            // can act on.
            false if real_samples.len() < 20 => real_samples.push(sample),
            _ => {}
        }
    }

    tracing::info!(
        walked,
        no_blob,
        distinct_blobs = claims.len(),
        duplicated = dupes.len(),
        with_synthetic,
        all_real,
        "duplicate-blob census complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "messages_walked": walked,
        // Rows with no file reference at all — a different defect,
        // repaired by `maintenance:repair-blob-refs`. Counted so this
        // census does not silently fold them in.
        "no_blob_ref": no_blob,
        "distinct_blobs": claims.len(),
        "duplicated_blobs": dupes.len(),
        "with_synthetic_id": with_synthetic,
        // **Two real ids on one file.** Non-zero means a second
        // writer exists and the monolith does not explain the stock.
        "both_ids_real": all_real,
        "samples_synthetic": samples,
        "samples_both_real": real_samples,
    }))
    .into_response()
}
