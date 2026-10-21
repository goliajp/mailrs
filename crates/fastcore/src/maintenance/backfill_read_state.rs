//! One-time reconciliation of read state between the maildir and the index.
//!
//! Run after the two write paths were fixed so nothing new accumulates
//! behind it.
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

mod message;
use message::reconcile_message;

#[derive(serde::Deserialize, Default)]
pub(crate) struct BackfillQuery {
    /// Report what would change without changing it.
    #[serde(default)]
    dry_run: bool,
}

/// What the whole run did, summed over every account.
#[derive(Default)]
struct Totals {
    walked: u64,
    already_agreed: u64,
    disk_marked_seen: u64,
    index_marked_seen: u64,
    thread_level_only_marked: u64,
    no_file: u64,
    errors: u64,
}

/// What one account had changed.
#[derive(Default)]
struct UserChanges {
    disk: u64,
    index: u64,
    thread: u64,
}

/// One account's pass: who, what its maildirs hold, and whether to write.
struct Pass<'a> {
    state: &'a FastcoreState,
    user: &'a str,
    // base id -> (maildir, flags)
    disk: HashMap<String, (Maildir, Vec<Flag>)>,
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

    let mut t = Totals::default();
    let mut by_user: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for user in &users {
        let pass = Pass {
            state: &state,
            user,
            disk: disk_flags_by_id(&state, user),
            dry_run: q.dry_run,
        };
        let mut u = UserChanges::default();
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            reconcile_thread(&pass, &tid, &mut t, &mut u);
        }

        if u.disk > 0 || u.index > 0 || u.thread > 0 {
            by_user.insert(
                user.clone(),
                serde_json::json!({
                    "disk_marked_seen": u.disk,
                    "index_marked_seen": u.index,
                    "thread_level_only_marked": u.thread,
                }),
            );
        }
    }

    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "walked": t.walked,
            "already_agreed": t.already_agreed,
            "changed": t.disk_marked_seen + t.index_marked_seen + t.thread_level_only_marked,
            "disk_marked_seen": t.disk_marked_seen,
            "index_marked_seen": t.index_marked_seen,
            "thread_level_only_marked": t.thread_level_only_marked,
            "no_file": t.no_file,
            "errors": t.errors,
            "by_user": by_user,
        }),
        motion.finish(&state),
    ))
    .into_response()
}

/// One filesystem pass per mailbox: base id -> (maildir, flags).
fn disk_flags_by_id(
    state: &Arc<FastcoreState>,
    user: &str,
) -> HashMap<String, (Maildir, Vec<Flag>)> {
    let mut disk = HashMap::new();
    for mb in crate::imap::backend::list_mailboxes(state, user) {
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
    disk
}

fn reconcile_thread(p: &Pass, tid: &str, t: &mut Totals, u: &mut UserChanges) {
    let (state, user) = (p.state, p.user);
    let mids = state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default();

    // The third case: the thread counter says read while no message
    // does. Decided before the per-message loop so the loop can act
    // on it.
    let thread_says_read = matches!(
        state.mailbox.get_thread_for_user(user, tid),
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
        if reconcile_message(p, mid, thread_level_only, t, u) {
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
        && !p.dry_run
        && matches!(
            state.mailbox.get_thread_for_user(user, tid),
            Ok(Some(ref row)) if row.unread_count == 0
        )
        && let Err(e) = state.mailbox.mark_seen(user, tid)
    {
        tracing::warn!(err = %e, %user, %tid, "backfill: axis clear failed");
    }
}
