//! Put each reader's own participant list back on their membership row.
//!
//! The row's `senders_csv` used to be copied from the shared thread hash,
//! which lists every sender any member received, so a member who joined a
//! conversation later listed people who never wrote to them. Stopping the
//! copy fixes new writes; the rows it already wrote stay wrong until this
//! runs.
//!
//! The right list is the senders of the messages the reader holds, oldest
//! first (`participants_from_messages`), which is also how the SQL core
//! builds it.
//!
//! Only conversations another account also holds, unless `all=true`. A
//! copied sender needs a second member to come from. On a conversation one
//! reader holds, the row can still differ from their messages (a message
//! removed before removals recounted the list, or a list the maildir heal
//! extended), but that is the reader's own mail and a separate decision.
//!
//! # Batching and convergence
//!
//! `skip` / `limit` walk the (user, conversation) pairs in a fixed order —
//! accounts sorted, each account's conversations sorted — so a long run is
//! a sequence of calls; `next_skip` is where the next one starts. Defaults
//! to a dry run. A row is written only when it differs, so a second run
//! over the same pairs reports `would_change: 0`.

use std::collections::{BTreeMap, BTreeSet};

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeParticipantsQuery {
    /// Restrict to one mailbox. Absent means every account.
    user: Option<String>,
    /// Report what would change without changing it. On unless asked off.
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    /// Conversations only this reader holds too; see the module docs.
    #[serde(default)]
    all: bool,
}

fn yes() -> bool {
    true
}

fn default_limit() -> usize {
    2000
}

#[derive(Default, serde::Serialize)]
struct UserTally {
    walked: u64,
    /// Walked pairs on a conversation another account also holds.
    shared: u64,
    agree: u64,
    change: u64,
    /// Of `change`: the row names a sender none of the reader's messages
    /// came from — the leak itself, rather than an order difference.
    names_a_stranger: u64,
    /// Rows only this reader holds, left as they are (`all=false`).
    kept_single_owner: u64,
}

/// The addresses a list names, compared the way the rest of the code
/// compares a mailbox, so `Carol <carol@x>` and `carol@x` are one person.
fn parts(csv: &str) -> BTreeSet<String> {
    mailrs_rfc5322::split_list(csv)
        .map(mailrs_rfc5322::addr_key)
        .filter(|p| !p.is_empty())
        .collect()
}

/// `POST /v1/admin/maintenance:recompute-thread-participants[?user=&dry_run=false&skip=&limit=&all=true]`
pub(crate) async fn recompute_thread_participants_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeParticipantsQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let users: Vec<String> = match &q.user {
        Some(u) => vec![u.clone()],
        None => match state.mailbox.list_account_addresses() {
            Ok(mut u) => {
                u.sort();
                u
            }
            Err(e) => {
                tracing::error!(err = %e, "list_account_addresses failed");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };

    let mut position = 0usize;
    let mut walked = 0u64;
    let mut agree = 0u64;
    let mut changed = 0u64;
    let mut row_missing = 0u64;
    let mut no_messages = 0u64;
    let mut errors = 0u64;
    // the first few changed conversations, to look at by hand
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut by_user: BTreeMap<String, UserTally> = BTreeMap::new();
    let end = q.skip.saturating_add(q.limit);

    'users: for user in &users {
        let mut tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        tids.sort();
        for tid in &tids {
            if position >= end {
                break 'users;
            }
            position += 1;
            if position <= q.skip {
                continue;
            }
            walked += 1;
            let tally = by_user.entry(user.clone()).or_default();
            tally.walked += 1;
            let shared = match state.mailbox.thread_message_owners(tid) {
                Ok(owners) => owners.iter().any(|(o, _)| o != user),
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "participants recompute: owners unread");
                    continue;
                }
            };
            if shared {
                tally.shared += 1;
            } else if !q.all {
                tally.kept_single_owner += 1;
                continue;
            }
            let have = match state.mailbox.stored_participants(user, tid) {
                Ok(Some(h)) => h,
                Ok(None) => {
                    row_missing += 1;
                    continue;
                }
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "participants recompute: row unread");
                    continue;
                }
            };
            if state
                .mailbox
                .user_thread_message_ids(user, tid)
                .map(|m| m.is_empty())
                .unwrap_or(false)
            {
                no_messages += 1;
                continue;
            }
            let want = match state.mailbox.participants_from_messages(user, tid) {
                Ok(w) => w,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "participants recompute: messages unread");
                    continue;
                }
            };
            if have == want {
                agree += 1;
                tally.agree += 1;
                continue;
            }
            tally.change += 1;
            let stranger = !parts(&have).is_subset(&parts(&want));
            if stranger {
                tally.names_a_stranger += 1;
            }
            if samples.len() < 20 {
                samples.push(serde_json::json!({
                    "user": user, "thread_id": tid, "shared": shared, "names_a_stranger": stranger,
                }));
            }
            if q.dry_run {
                continue;
            }
            match state.mailbox.recount_participants(user, tid, false) {
                Ok(true) => changed += 1,
                Ok(false) => {}
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "participants recompute: write failed");
                }
            }
        }
    }

    let would_change: u64 = by_user.values().map(|t| t.change).sum();
    let names_a_stranger: u64 = by_user.values().map(|t| t.names_a_stranger).sum();
    let done = walked < q.limit as u64;
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        agree,
        would_change,
        changed,
        "thread participants recompute"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "all": q.all,
            "skip": q.skip,
            "limit": q.limit,
            // What it looked at, so a zero below reads as agreement rather
            // than as an empty walk.
            "walked": walked,
            "agree": agree,
            "would_change": would_change,
            "names_a_stranger": names_a_stranger,
            "changed": changed,
            "row_missing": row_missing,
            "no_messages": no_messages,
            "errors": errors,
            "by_user": by_user,
            "samples": samples,
            "done": done,
            "next_skip": q.skip + walked as usize,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
