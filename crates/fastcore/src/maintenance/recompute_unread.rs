//! Put each reader's `unread` column back to what their own messages say.
//!
//! The column decides the Unread list and the badge. A split used to take
//! a message's read state from its shared blob, which one holder's read had
//! set for every holder, so a split conversation could carry an unread
//! message and still be missing from its reader's Unread list. The write
//! paths are fixed; this corrects the rows already written.
//!
//! The answer is the aggregate index over the reader's message rows
//! (`recount_unread_column`). Only conversations another account also
//! holds, unless `all=true`: a single reader's column can disagree with
//! their rows for reasons of their own (a read recorded only on the column
//! before both halves were written together), and turning those unread
//! again is a separate decision.
//!
//! `skip` / `limit` walk the (user, conversation) pairs accounts sorted,
//! then conversations sorted; `next_skip` is where the next call starts.
//! Dry run unless `dry_run=false`. A row is written only when it differs,
//! so the run after a real one reports `would_change: 0`.

use std::collections::BTreeMap;

use mailrs_mailbox_kevy::UnreadColumn;

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeUnreadQuery {
    user: Option<String>,
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
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
    shared: u64,
    agree: u64,
    /// Would join the Unread list and the badge.
    turns_unread: u64,
    /// Would leave them.
    turns_read: u64,
    index_silent: u64,
    kept_single_owner: u64,
}

/// `POST /v1/admin/maintenance:recompute-unread-column[?user=&dry_run=false&skip=&limit=&all=true]`
pub(crate) async fn recompute_unread_column_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeUnreadQuery>,
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
    let mut changed = 0u64;
    let mut errors = 0u64;
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
                    tracing::warn!(err = %e, %user, %tid, "unread recompute: owners unread");
                    continue;
                }
            };
            if shared {
                tally.shared += 1;
            } else if !q.all {
                tally.kept_single_owner += 1;
                continue;
            }
            let verdict = match state.mailbox.recount_unread_column(user, tid, q.dry_run) {
                Ok(v) => v,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "unread recompute failed");
                    continue;
                }
            };
            let turned = match verdict {
                UnreadColumn::Agrees => {
                    tally.agree += 1;
                    continue;
                }
                UnreadColumn::IndexSilent | UnreadColumn::NoRow => {
                    tally.index_silent += 1;
                    continue;
                }
                UnreadColumn::TurnsUnread => {
                    tally.turns_unread += 1;
                    "unread"
                }
                UnreadColumn::TurnsRead => {
                    tally.turns_read += 1;
                    "read"
                }
            };
            if !q.dry_run {
                changed += 1;
            }
            if samples.len() < 20 {
                samples.push(serde_json::json!({
                    "user": user, "thread_id": tid, "shared": shared, "turns": turned,
                }));
            }
        }
    }

    let turns_unread: u64 = by_user.values().map(|t| t.turns_unread).sum();
    let turns_read: u64 = by_user.values().map(|t| t.turns_read).sum();
    let done = walked < q.limit as u64;
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        turns_unread,
        turns_read,
        changed,
        "unread column recompute"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "all": q.all,
            "skip": q.skip,
            "limit": q.limit,
            "walked": walked,
            "would_change": turns_unread + turns_read,
            "turns_unread": turns_unread,
            "turns_read": turns_read,
            "changed": changed,
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
