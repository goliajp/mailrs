//! Apply the fraud checks to mail that arrived before they existed.
//!
//! `mailrs-fraud` runs at receive time, so a signal added on Tuesday
//! reaches Wednesday's mail and never touches Monday's. The wave this
//! was written for had been landing for months: fifty-odd messages
//! sitting in inboxes, every one of which the new checks would have
//! caught.
//!
//! # Dry by default
//!
//! The first run reports and changes nothing. That is not politeness —
//! it is the only way to see what a new signal would have done to a
//! real mailbox before it does it, and this repository has a rule about
//! it (`measure-before-you-cut-over`). Pass `dry_run=false` to move
//! them.
//!
//! # Bounded, and it pauses
//!
//! Same shape as `backfill-decode-headers` and for the same reason: an
//! unbounded sweep over this mailbox took the mail service down for
//! half an hour on 2026-08-26. `limit` threads per call, a pause every
//! `PAUSE_EVERY`, and `next_skip` in the answer.
//!
//! # What it reads
//!
//! The newest message of each thread, from that user's own maildir
//! file. The display name has to be decoded before it can be compared —
//! the names arrive base64'd — and the `X-Mailer` is read raw.

use std::collections::HashMap;

use super::prelude::*;

/// Threads between pauses.
const PAUSE_EVERY: u64 = 25;

#[derive(serde::Deserialize)]
pub(crate) struct RescanQuery {
    /// Report without moving anything. **Default true.**
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
    500
}
fn default_pause_ms() -> u64 {
    50
}

/// `POST /v1/admin/maintenance:fraud-rescan?dry_run=false`
pub(crate) async fn fraud_rescan_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RescanQuery>,
) -> axum::response::Response {
    let policy = mailrs_fraud::Policy {
        org_names: csv_env("MAILRS_ORG_NAMES"),
        our_domains: csv_env("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
    };
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut stopped_early = false;
    let mut found = 0u64;
    let mut already_junk = 0u64;
    let mut moved = 0u64;
    let mut no_file = 0u64;
    // Which check fired, because "12 found" does not say whether the
    // one that needs an allow-list entry is among them.
    let mut by_reason: HashMap<String, u64> = HashMap::new();
    let mut samples: Vec<serde_json::Value> = Vec::new();

    'walk: for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
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

            let Some(raw) = newest_raw(&state, user, &tid) else {
                no_file += 1;
                continue;
            };
            let findings = mailrs_fraud::scan(
                &mailrs_inbound::from_header(&raw),
                x_mailer(&raw).as_deref(),
                &policy,
            );
            if !findings.any() {
                continue;
            }
            found += 1;
            for r in mailrs_fraud::reasons(findings) {
                *by_reason.entry(r.to_string()).or_default() += 1;
            }
            if samples.len() < 25 {
                samples.push(serde_json::json!({
                    "user": user,
                    "thread": tid,
                    "from": mailrs_inbound::from_header(&raw),
                    "reasons": mailrs_fraud::reasons(findings),
                    "score": mailrs_fraud::score(findings),
                }));
            }
            if q.dry_run {
                continue;
            }
            match state.mailbox.set_junk(user, &tid, true) {
                // `false` means the row already said Junk — worth its
                // own count, or a re-run reads as having moved things
                // it did not.
                Ok(true) => moved += 1,
                Ok(false) => already_junk += 1,
                Err(e) => tracing::warn!(err = %e, %user, %tid, "fraud rescan: set_junk failed"),
            }
        }
    }

    tracing::info!(
        walked,
        found,
        moved,
        already_junk,
        no_file,
        dry_run = q.dry_run,
        "fraud-rescan complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "threads_walked": walked,
        "found": found,
        "moved_to_junk": moved,
        "already_junk": already_junk,
        "no_file": no_file,
        "by_reason": by_reason,
        "samples": samples,
    }))
    .into_response()
}

/// A comma-separated environment variable, or nothing.
///
/// Read here rather than threaded through `FastcoreState`: this is the
/// same process the receiver's policy is configured for, and a second
/// copy of the values is a second thing to keep in step.
fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The raw bytes of a thread's newest message, from this user's copy.
fn newest_raw(state: &Arc<FastcoreState>, user: &str, tid: &str) -> Option<Vec<u8>> {
    let mut newest: Option<(i64, String)> = None;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
    {
        let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
            continue;
        };
        let Ok(wire) =
            serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
        else {
            continue;
        };
        if wire.blob_ref.is_empty() {
            continue;
        }
        let better = match &newest {
            None => true,
            Some((d, _)) => wire.date >= *d,
        };
        if better {
            newest = Some((wire.date, wire.blob_ref));
        }
    }
    let (_, blob_ref) = newest?;
    read_maildir_file(user, &blob_ref)
}

/// The `X-Mailer` header value, unfolded far enough to compare.
fn x_mailer(raw: &[u8]) -> Option<String> {
    let head = &raw[..raw.len().min(16 * 1024)];
    let text = String::from_utf8_lossy(head);
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("x-mailer:") {
            return Some(line[line.len() - rest.len()..].trim().to_string());
        }
    }
    None
}
