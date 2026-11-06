//! `POST /v1/admin/maintenance:settle-own-unseen` — set `\Seen` on own
//! mail stored without it, so that counting own unseen mail as unread
//! turns no conversation unread that its reader did not mark unread.
//!
//! The rule and the one exception are on
//! `KevyMailboxStore::settle_own_unseen`. Run it once after the release
//! that counts own unseen mail as unread, before `group-backfill` or
//! `recompute-unread-column`, which rewrite the rows and would otherwise
//! regroup them first.
//!
//! Dry run unless `dry_run=false`. `skip` / `limit` walk the (user,
//! conversation) pairs accounts sorted, then conversations sorted;
//! `next_skip` is where the next call starts. `threads=true` lists every
//! conversation that has own unseen mail, for comparing what the list
//! shows before and after.

use std::collections::BTreeMap;

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct SettleOwnUnseenQuery {
    user: Option<String>,
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    threads: bool,
}

fn yes() -> bool {
    true
}

fn default_limit() -> usize {
    5000
}

#[derive(Default, serde::Serialize)]
struct UserTally {
    walked: u64,
    /// Own messages without `\Seen`.
    own_unseen_rows: u64,
    /// Conversations holding at least one.
    threads_with_own_unseen: u64,
    /// No unread mail under the old rule, some under the new one with the
    /// rows as they are: what regrouping them would turn unread.
    turn_unread_as_is: u64,
    /// Of those, the ones not in the Unread list: nobody marked them.
    turn_unread_as_is_not_marked: u64,
    /// No unread mail under the old rule, some once the step has run.
    /// Only conversations in the Unread list can be here.
    turn_unread_after: u64,
    turn_unread_after_not_marked: u64,
    /// Rows the step sets `\Seen` on.
    set_seen: u64,
    /// Kept unseen as the reader's own mark.
    kept: u64,
}

pub(crate) async fn settle_own_unseen_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<SettleOwnUnseenQuery>,
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
    let mut errors = 0u64;
    let mut renamed = 0u64;
    let mut rename_failed = 0u64;
    let mut listed: Vec<serde_json::Value> = Vec::new();
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
            let found = match state.mailbox.settle_own_unseen(user, tid, !q.dry_run) {
                Ok(f) => f,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "settle own unseen failed");
                    continue;
                }
            };
            if found.rows == 0 {
                continue;
            }
            tally.own_unseen_rows += found.rows;
            tally.threads_with_own_unseen += 1;
            tally.set_seen += found.seen.len() as u64;
            tally.kept += found.rows - found.seen.len() as u64;
            let turns_as_is = found.before == 0 && found.as_is > 0;
            let turns_after = found.before == 0 && found.after > 0;
            tally.turn_unread_as_is += u64::from(turns_as_is);
            tally.turn_unread_as_is_not_marked += u64::from(turns_as_is && !found.marked_unread);
            tally.turn_unread_after += u64::from(turns_after);
            tally.turn_unread_after_not_marked += u64::from(turns_after && !found.marked_unread);
            if !q.dry_run {
                for (blob_ref, flags) in &found.seen {
                    match crate::maildir_scan::apply_flag_bitmask(user, blob_ref, *flags) {
                        Ok(true) => renamed += 1,
                        Ok(false) => {}
                        Err(e) => {
                            rename_failed += 1;
                            tracing::warn!(err = %e, %user, %blob_ref, "settle own unseen: rename failed");
                        }
                    }
                }
            }
            if q.threads {
                listed.push(serde_json::json!({
                    "user": user,
                    "thread_id": tid,
                    "rows": found.rows,
                    "before": found.before,
                    "as_is": found.as_is,
                    "after": found.after,
                    "marked_unread": found.marked_unread,
                }));
            }
        }
    }

    let sum = |f: fn(&UserTally) -> u64| by_user.values().map(f).sum::<u64>();
    let set_seen = sum(|t| t.set_seen);
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        set_seen,
        renamed,
        "settle own unseen"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "skip": q.skip,
            "limit": q.limit,
            "walked": walked,
            "own_unseen_rows": sum(|t| t.own_unseen_rows),
            "threads_with_own_unseen": sum(|t| t.threads_with_own_unseen),
            "turn_unread_as_is": sum(|t| t.turn_unread_as_is),
            "turn_unread_as_is_not_marked": sum(|t| t.turn_unread_as_is_not_marked),
            "turn_unread_after": sum(|t| t.turn_unread_after),
            "turn_unread_after_not_marked": sum(|t| t.turn_unread_after_not_marked),
            "set_seen": set_seen,
            "kept": sum(|t| t.kept),
            "files_renamed": renamed,
            "rename_failed": rename_failed,
            "errors": errors,
            "by_user": by_user,
            "threads": listed,
            "done": walked < q.limit as u64,
            "next_skip": q.skip + walked as usize,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
