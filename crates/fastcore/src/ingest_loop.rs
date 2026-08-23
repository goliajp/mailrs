//! The periodic pass: mirror from a core-api server, then heal from
//! the maildir.
//!
//! Split from `ingest.rs` at the file-size gate. The seam is between
//! *when* mail is looked for and *what filing one means* — nothing
//! here knows how a message becomes a thread, and nothing there knows
//! about a timer.

use std::sync::Arc;

use crate::maildir_scan::healed_from_maildir;
use crate::{FastcoreState, now_secs};

/// Periodic sync loop. Two jobs on the same tick:
///
/// 1. OPTIONAL ingest: when `MAILRS_CORE_RPC_BASE` + the shared secret
///    are set, poll that core-api server for threads newer than the
///    per-user cursor and mirror them in (the monolith-era cutover
///    path).
/// 2. MANDATORY maildir self-heal: thread/message/uid repair straight
///    from disk. This must run regardless of the ingest config —
///    returning early when MAILRS_CORE_RPC_BASE was unset silently
///    killed self-heal on the first monolith-free deploy and new
///    inbound mail stopped appearing in the UI (2026-07-04, 99-message
///    backlog on prod before the stopgap).
pub(crate) async fn ingest_sync_loop(state: Arc<FastcoreState>) {
    let client = match (
        std::env::var("MAILRS_CORE_RPC_BASE"),
        std::env::var(mailrs_core_api::AUTH_SECRET_ENV),
    ) {
        (Ok(base), Ok(secret)) => Some(mailrs_core_api::client::Client::new(base, secret)),
        _ => {
            tracing::info!("no ingest source configured — running maildir self-heal only");
            None
        }
    };
    let interval = std::time::Duration::from_secs(
        std::env::var("MAILRS_FASTCORE_SYNC_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30),
    );
    // Self-heal pacing. Every writer of a maildir file now indexes it
    // write-through (spool_drain, bounce, IMAP APPEND/COPY, REST
    // copy/move), so the only gap left for this sweep is a process that
    // died between the file landing and the index write. Those files are
    // necessarily recent, so the routine pass only inspects names newer
    // than INCREMENTAL_WINDOW and costs a readdir instead of ~48k header
    // reads (staging 2026-07-19).
    //
    // A full pass still runs at boot and once a day, to catch anything a
    // clock skew or an out-of-band file drop put outside the window.
    // Backoff on top: each idle round doubles the wait, any repair
    // resets it to the base interval.
    const MAX_IDLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(900);
    const FULL_SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    // Generous relative to the crash window it covers; the cost of a
    // wider window is only extra header reads on files we then skip.
    const INCREMENTAL_WINDOW_SECS: i64 = 6 * 3600;
    let mut idle_rounds: u32 = 0;
    let mut last_full_sweep: Option<std::time::Instant> = None;
    loop {
        let mut wait = interval;
        match &client {
            Some(c) => {
                if let Err(e) = run_ingest_once(&state, c).await {
                    tracing::warn!(error = %e, "ingest sync tick failed");
                }
            }
            None => {
                let full = last_full_sweep.is_none_or(|t| t.elapsed() >= FULL_SWEEP_EVERY);
                let since = match full {
                    true => 0,
                    false => now_secs().saturating_sub(INCREMENTAL_WINDOW_SECS),
                };
                let addrs = state.mailbox.list_account_addresses().unwrap_or_default();
                let mut repaired = false;
                for user in &addrs {
                    repaired |= healed_from_maildir(&state, user, since).await;
                }
                if full {
                    last_full_sweep = Some(std::time::Instant::now());
                }
                if repaired {
                    idle_rounds = 0;
                } else {
                    idle_rounds = idle_rounds.saturating_add(1);
                }
                let backoff = interval
                    .saturating_mul(1u32 << idle_rounds.min(6))
                    .min(MAX_IDLE_INTERVAL);
                wait = backoff;
            }
        }
        tokio::time::sleep(wait).await;
    }
}

pub(crate) async fn run_ingest_once(
    state: &Arc<FastcoreState>,
    client: &mailrs_core_api::client::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    use mailrs_core_api::method::conversation::ListConversationsRequest;
    use mailrs_core_api::types::ConversationFilter;

    let addrs = state.mailbox.list_account_addresses()?;
    for user in &addrs {
        let cursor_key = format!("mailrs:sync:cursor:{user}");
        let prev = state
            .mailbox
            .store_ref()
            .get(cursor_key.as_bytes())?
            .and_then(|b| String::from_utf8_lossy(&b).parse::<i64>().ok())
            .unwrap_or(0);
        let req = ListConversationsRequest {
            filter: ConversationFilter {
                accounts: None,
                limit: 200,
                before_ts: None,
                category: None,
                domains: None,
                archived: false,
                folder: None,
                unread: None,
                starred: None,
                section: None,
            },
        };
        // Try monolith. If it's down, skip the ingest step but STILL
        // run the maildir-based self-heal at the bottom of the loop —
        // fastcore's whole point is to work without monolith.
        let resp_opt = match client.list_conversations(user, &req).await {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(error = %e, %user, "monolith list_conversations failed (continuing to self-heal from maildir)");
                None
            }
        };
        let resp = match resp_opt {
            Some(r) => r,
            None => {
                // core RPC unavailable — full sweep, this path is rare
                healed_from_maildir(state, user, 0).await;
                continue;
            }
        };
        let mut max_seen = prev;
        let mut newly = 0;
        for s in &resp.items {
            if s.last_date <= prev {
                continue;
            }
            // If the thread already exists in kevy, don't clobber the
            // aggregate (fastcore-side mark_read / pin / archive stay
            // sticky) — but DO diff messages, because a thread with a
            // new reply gets its `last_date` bumped and needs the new
            // message body ingested. Prior version skipped the whole
            // packet, so new replies never appeared until the user
            // re-imported.
            let already_exists = matches!(state.mailbox.get_thread(&s.thread_id), Ok(Some(_)));
            if already_exists {
                if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                    for w in &msgs.items {
                        // Only write if we don't already have this
                        // message id (prevents duplicate writes on
                        // every sync tick).
                        if state
                            .mailbox
                            .get_message(&w.message_id)
                            .ok()
                            .flatten()
                            .is_some()
                        {
                            continue;
                        }
                        let payload = match serde_json::to_vec(w) {
                            Ok(p) => p,
                            Err(_) => continue,
                        };
                        let _ = state.mailbox.upsert_user_message(
                            user,
                            &s.thread_id,
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
                        let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                    }
                }
                max_seen = max_seen.max(s.last_date);
                continue;
            }
            let row = mailrs_mailbox_kevy::ThreadRow {
                account_id: String::new(),
                thread_id: s.thread_id.clone(),
                subject: s.subject.clone(),
                senders_csv: s.participants.clone(),
                count: s.message_count as i64,
                unread_count: s.unread_count as i64,
                latest_date: s.last_date,
                latest_preview: s.snippet.clone(),
                category: s.category.clone(),
                importance_level: s.importance_level.clone(),
                importance_score: s.importance_score as f64,
                requires_action: s.requires_action,
                pinned: s.pinned,
                archived: s.archived,
                has_action: s.requires_action,
                sent_count: s.sent_count as i64,
                starred: s.flagged,
                // Not carried by an arrival: `upsert_thread` derives
                // the membership row from the shared aggregate, and a
                // snooze is one reader's — writing it from here would
                // reset it on the next message. `thread_user_pairs`
                // omits it for the same reason it omits `starred`.
                snoozed_until: 0,
            };
            if let Err(e) = state.mailbox.upsert_thread(user, &row) {
                tracing::warn!(error = %e, %user, tid = %s.thread_id, "upsert_thread failed");
                continue;
            }
            // Pull the thread's messages and mirror them so `get_thread_messages`
            // returns the fresh content on the next click.
            if let Ok(msgs) = client.list_thread_messages(user, &s.thread_id).await {
                for w in &msgs.items {
                    let payload = match serde_json::to_vec(w) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let _ = state.mailbox.upsert_user_message(
                        user,
                        &s.thread_id,
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
                    let _ = state.mailbox.index_uid(user, w.uid, &w.message_id);
                }
            }
            max_seen = max_seen.max(s.last_date);
            newly += 1;
        }
        if newly > 0 {
            state
                .mailbox
                .store_ref()
                .set(cursor_key.as_bytes(), max_seen.to_string().as_bytes())?;
            tracing::info!(%user, newly, cursor = max_seen, "ingest sync applied");
        }

        // Self-heal pass — reads maildir directly, no monolith call.
        //
        // Fastcore's whole reason for existing is to be spg-independent.
        // If we heal by calling monolith, then a spg outage takes
        // fastcore down with it — defeats the point. Instead, walk the
        // user's maildir(s), parse each file's headers, and upsert any
        // messages whose thread_id already exists in fastcore but has
        // an empty messages zset.
        healed_from_maildir(state, user, 0).await;
    }
    Ok(())
}
