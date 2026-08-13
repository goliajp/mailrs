//! One-time reconciliation of read state between the maildir and the index.
//!
//! Step 2 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`, run after
//! the two write paths were fixed so nothing new accumulates behind it.
//!
//! # Which way the facts travel here, and why it is not the steady rule
//!
//! The design makes the **maildir authoritative**: on disagreement the file
//! name wins and the index is rebuilt from it. This route does that in one
//! direction and the *opposite* in the other, deliberately:
//!
//! - **disk says read, index does not** → the index is corrected. That is
//!   the steady-state rule.
//! - **index says read, disk does not** → the **file** is corrected. Those
//!   reads are real — a person opened the mail in the web UI — and until
//!   the write path was fixed they were only ever recorded in the index. A
//!   backfill that let the maildir win here would not be enforcing the rule,
//!   it would be **deleting 14,704 facts** because they were written to the
//!   wrong place.
//!
//! Measured on production before this ran: `seen_only_in_index` 14,704,
//! `seen_only_on_disk` 0. So in practice the second branch is the whole job
//! and the first is there for correctness going forward.
//!
//! # The third case
//!
//! A thread whose `unread_count` is 0 while every one of its messages still
//! reads unread. `mark_seen` wrote the thread-level counter and touched no
//! message, so the only record of the read is a count — nothing per-message,
//! on either side. 999 threads on production, the gap between
//! `unread_count_differs` (15,703) and `seen_only_in_index` (14,704).
//!
//! Resolving these needs the definition this RFC settles: **a thread being
//! read means every message in it carries `S`**, which is what an IMAP
//! client shows and what the file name can express. So they are given `S`.
//!
//! Idempotent, and reports what it walked separately from what it changed:
//! a second run must say `changed: 0`.

use std::collections::{BTreeMap, HashMap};

use mailrs_core_api::method::message::{FLAG_SEEN, MessageWire};
use mailrs_maildir::{Flag, Maildir};

use super::prelude::*;

#[derive(serde::Deserialize, Default)]
pub(crate) struct BackfillQuery {
    /// Report what would change without changing it.
    #[serde(default)]
    dry_run: bool,
}

/// `POST /v1/admin/maintenance:read-state-backfill?dry_run=true`
pub(crate) async fn read_state_backfill_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut walked = 0u64;
    let mut already_agreed = 0u64;
    let mut disk_marked_seen = 0u64;
    let mut index_marked_seen = 0u64;
    let mut thread_level_only_marked = 0u64;
    let mut no_file = 0u64;
    let mut errors = 0u64;
    let mut by_user: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for user in &users {
        // One filesystem pass per mailbox: base id -> (maildir, flags).
        let mut disk: HashMap<String, (Maildir, Vec<Flag>)> = HashMap::new();
        for mb in crate::imap::backend::list_mailboxes(&state, user) {
            let md = Maildir::open(&mb.path);
            let entries = md
                .scan_new()
                .unwrap_or_default()
                .into_iter()
                .chain(md.scan_cur().unwrap_or_default());
            for e in entries {
                disk.insert(e.id.0, (Maildir::open(&mb.path), e.flags));
            }
        }

        let mut u_disk = 0u64;
        let mut u_index = 0u64;
        let mut u_thread = 0u64;

        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            let mids = state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default();

            // The third case: the thread counter says read while no message
            // does. Decided before the per-message loop so the loop can act
            // on it.
            let thread_says_read = matches!(
                state.mailbox.get_thread_for_user(user, &tid),
                Ok(Some(ref row)) if row.unread_count == 0
            );
            let mut any_message_says_read = false;
            for mid in &mids {
                if let Ok(Some(f)) = state.mailbox.user_message_facts(user, mid)
                    && f.flags & FLAG_SEEN != 0
                {
                    any_message_says_read = true;
                    break;
                }
            }
            let thread_level_only = thread_says_read && !any_message_says_read && !mids.is_empty();

            for mid in &mids {
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, mid) else {
                    continue;
                };
                if facts.blob_ref.is_empty() {
                    no_file += 1;
                    continue;
                }
                let Some((_, disk_flags)) = disk.get(base_id(&facts.blob_ref)) else {
                    no_file += 1;
                    continue;
                };
                walked += 1;

                let disk_seen = disk_flags.contains(&Flag::Seen);
                let index_seen = facts.flags & FLAG_SEEN != 0;
                // A thread read at thread level counts as read for every
                // message in it, per the definition above.
                let index_seen = index_seen || thread_level_only;

                if disk_seen == index_seen {
                    already_agreed += 1;
                    continue;
                }

                if q.dry_run {
                    match (disk_seen, index_seen) {
                        (false, true) if thread_level_only => thread_level_only_marked += 1,
                        (false, true) => disk_marked_seen += 1,
                        _ => index_marked_seen += 1,
                    }
                    continue;
                }

                if index_seen {
                    // The file is behind: write `S`, keeping every flag the
                    // bitmask cannot express (`P`), which is why this goes
                    // through the shared helper rather than `mark_processed`.
                    match crate::maildir_scan::apply_flag_bitmask(
                        user,
                        &facts.blob_ref,
                        facts.flags | FLAG_SEEN,
                    ) {
                        Ok(true) if thread_level_only => {
                            thread_level_only_marked += 1;
                            u_thread += 1;
                        }
                        Ok(true) => {
                            disk_marked_seen += 1;
                            u_disk += 1;
                        }
                        Ok(false) => already_agreed += 1,
                        Err(e) => {
                            tracing::warn!(err = %e, %user, %mid, "backfill: rename failed");
                            errors += 1;
                        }
                    }
                    // The index row's own bit may still be clear on a
                    // thread-level-only thread; bring it in line so the
                    // shadow converges and the axis is right.
                    if thread_level_only && facts.flags & FLAG_SEEN == 0 {
                        set_index_seen(&state, user, facts.uid, facts.flags | FLAG_SEEN);
                    }
                } else {
                    // The disk is ahead — the steady-state direction.
                    set_index_seen(&state, user, facts.uid, facts.flags | FLAG_SEEN);
                    index_marked_seen += 1;
                    u_index += 1;
                }
            }
        }

        if u_disk > 0 || u_index > 0 || u_thread > 0 {
            by_user.insert(
                user.clone(),
                serde_json::json!({
                    "disk_marked_seen": u_disk,
                    "index_marked_seen": u_index,
                    "thread_level_only_marked": u_thread,
                }),
            );
        }
    }

    Json(serde_json::json!({
        "dry_run": q.dry_run,
        "walked": walked,
        "already_agreed": already_agreed,
        "changed": disk_marked_seen + index_marked_seen + thread_level_only_marked,
        "disk_marked_seen": disk_marked_seen,
        "index_marked_seen": index_marked_seen,
        "thread_level_only_marked": thread_level_only_marked,
        "no_file": no_file,
        "errors": errors,
        "by_user": by_user,
    }))
    .into_response()
}

/// Set `\Seen` on one per-user message row, going through the same index
/// sync the flags route uses so the thread counter and the engagement
/// signal follow.
fn set_index_seen(state: &Arc<FastcoreState>, user: &str, uid: u32, flags: u32) {
    let Ok(Some(bytes)) = state.mailbox.get_message_by_uid(user, uid) else {
        return;
    };
    let Ok(mut wire) = serde_json::from_slice::<MessageWire>(&bytes) else {
        return;
    };
    let old = wire.flags;
    wire.flags = flags;
    let Ok(json) = serde_json::to_vec(&wire) else {
        return;
    };
    if let Err(e) = crate::routes::message_ops::sync_index_to_flags(state, user, &wire, &json, old)
    {
        tracing::warn!(err = %e, %user, uid, "backfill: index sync failed");
    }
}

/// Same rule as the shadow's: a `blob_ref` loses its subfolder prefix and
/// its flag suffix to give the id the disk map is keyed by.
fn base_id(blob_ref: &str) -> &str {
    let file = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    file.split(':').next().unwrap_or(file)
}
