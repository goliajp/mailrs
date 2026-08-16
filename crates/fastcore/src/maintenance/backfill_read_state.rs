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

use mailrs_core_api::method::message::FLAG_SEEN;
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
    // What the store did while this ran — see `store_motion`.
    let motion = crate::store_motion::begin(&state);
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
            let mut thread_touched = false;

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
                let row_seen = facts.flags & FLAG_SEEN != 0;
                // A thread read at thread level counts as read for every
                // message in it, per the definition above.
                //
                // Only for deciding what to write to the **file**. Comparing
                // it against the disk instead is what made this route unable
                // to converge its own shadow: a message the disk already
                // called read matched the thread's verdict, was counted as
                // agreement, and kept a clear index bit forever — 200 of the
                // 215 `seen_only_on_disk` rows on production, none of them
                // reachable by any number of runs.
                let index_seen = row_seen || thread_level_only;

                if disk_seen && !row_seen {
                    // The disk is ahead of the row, whatever the thread
                    // counter believes. The file name is the authority.
                    //
                    // Counted on the write, not before it: the previous
                    // version incremented first and wrote through a helper
                    // that gives up silently when the uid index cannot
                    // answer — which is every one of these rows, since they
                    // carry `uid: 0`. It reported 215 repaired on every run
                    // and repaired none.
                    if q.dry_run {
                        index_marked_seen += 1;
                    } else {
                        match state.mailbox.mark_user_message_seen(user, mid) {
                            Ok(Some(_)) => {
                                index_marked_seen += 1;
                                u_index += 1;
                                thread_touched = true;
                            }
                            Ok(None) => already_agreed += 1,
                            Err(e) => {
                                tracing::warn!(err = %e, %user, %mid, "backfill: row write failed");
                                errors += 1;
                            }
                        }
                    }
                    continue;
                }

                if disk_seen == index_seen {
                    already_agreed += 1;
                    continue;
                }

                // Only one case is left: the index says read and the file
                // does not. The disk-ahead direction returned above.
                if q.dry_run {
                    if thread_level_only {
                        thread_level_only_marked += 1;
                    } else {
                        disk_marked_seen += 1;
                    }
                    continue;
                }

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
                if thread_level_only
                    && !row_seen
                    && matches!(state.mailbox.mark_user_message_seen(user, mid), Ok(Some(_)))
                {
                    thread_touched = true;
                }
            }

            // Drop the thread off the unread axis when nothing is left
            // unread.
            //
            // There used to be a `repair_thread_counts` here first, to
            // recompute the counter the bits above are a derivation of.
            // The counter is no longer a stored number — the declared
            // index derives it from these very rows — so recomputing it
            // is recomputing nothing, and the call went with the rest of
            // the repair machinery (C5c). The axis column is a separate
            // fact and still hand-maintained, so clearing it stays.
            if thread_touched
                && !q.dry_run
                && matches!(
                    state.mailbox.get_thread_for_user(user, &tid),
                    Ok(Some(ref row)) if row.unread_count == 0
                )
                && let Err(e) = state.mailbox.mark_seen(user, &tid)
            {
                tracing::warn!(err = %e, %user, %tid, "backfill: axis clear failed");
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

    Json(crate::store_motion::with_motion(
        serde_json::json!({
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
        }),
        motion.finish(&state),
    ))
    .into_response()
}

/// Same rule as the shadow's: a `blob_ref` loses its subfolder prefix and
/// its flag suffix to give the id the disk map is keyed by.
fn base_id(blob_ref: &str) -> &str {
    let file = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    file.split(':').next().unwrap_or(file)
}
