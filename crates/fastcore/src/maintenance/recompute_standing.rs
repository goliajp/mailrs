//! Put each reader's own tab and time back on their membership row, and
//! count, per account, how many conversations that moves.
//!
//! The row's `category` / `bucket` and `activity` used to be copied from the
//! shared thread hash, which holds whichever member's mail arrived or
//! whichever member filed the conversation last. Stopping the copy fixes new
//! writes; the rows already written stay as they are until this runs
//! (`recount_standing`).
//!
//! The activity is recounted from the reader's own messages. The category
//! is recounted only where the reader's newest message has a recorded
//! category (stored with the message since this change, or the
//! per-recipient placement record before it); elsewhere the row keeps its
//! own and `category_unknown` counts it. A reader's own earlier move is not
//! recorded on old rows, so a move counted here can undo one: the samples
//! and `tab_moves` are what to look at before a real run.
//!
//! Only conversations another account also holds, unless `all=true`.
//! `skip` / `limit` walk (user, conversation) pairs in a fixed order;
//! `next_skip` is where the next call starts. Dry run unless
//! `dry_run=false`. A row is written only when it differs, so the run after
//! a real one reports `would_change: 0`.

use std::collections::BTreeMap;

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeStandingQuery {
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
    kept_single_owner: u64,
    no_messages: u64,
    agree: u64,
    change: u64,
    /// Of `change`: the conversation would move to another tab, by
    /// `from->to` bucket.
    tab_moves: u64,
    tab_moves_by: BTreeMap<String, u64>,
    /// Of `change`: the category changes within one tab.
    category_only: u64,
    /// Of `change`: the sort key changes, so the list re-sorts; earlier
    /// is what mail the reader never received had pushed up.
    resorts: u64,
    resorts_earlier: u64,
    resorts_later: u64,
    /// The reader's newest message has no recorded category.
    category_unknown: u64,
    /// The row asks for action; nothing records whose, so it is counted
    /// and not changed.
    requires_action_set: u64,
}

fn bucket(category: &str) -> &'static str {
    mailrs_mailbox_kevy::keys::bucket_of(category).name()
}

/// `POST /v1/admin/maintenance:recompute-thread-standing[?user=&dry_run=false&skip=&limit=&all=true]`
pub(crate) async fn recompute_thread_standing_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeStandingQuery>,
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
            let owners = match state.mailbox.thread_message_owners(tid) {
                Ok(o) => o,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "standing recompute: owners unread");
                    continue;
                }
            };
            let shared = owners.iter().any(|(o, _)| o != user);
            if shared {
                tally.shared += 1;
            } else if !q.all {
                tally.kept_single_owner += 1;
                continue;
            }
            if let Ok(Some(row)) = state.mailbox.get_thread_for_user(user, tid)
                && row.requires_action
            {
                tally.requires_action_set += 1;
            }
            let r = match state.mailbox.recount_standing(user, tid, q.dry_run) {
                Ok(r) => r,
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "standing recompute failed");
                    continue;
                }
            };
            if !r.held {
                tally.no_messages += 1;
                continue;
            }
            if r.category_to.is_none() {
                tally.category_unknown += 1;
            }
            if !r.differs {
                tally.agree += 1;
                continue;
            }
            tally.change += 1;
            let to = r.category_to.as_deref().unwrap_or(&r.category_from);
            let (from_tab, to_tab) = (bucket(&r.category_from), bucket(to));
            if from_tab != to_tab {
                tally.tab_moves += 1;
                *tally
                    .tab_moves_by
                    .entry(format!("{from_tab}->{to_tab}"))
                    .or_default() += 1;
            } else if to != r.category_from {
                tally.category_only += 1;
            }
            if r.activity_to != r.activity_from {
                tally.resorts += 1;
                match r.activity_to < r.activity_from {
                    true => tally.resorts_earlier += 1,
                    false => tally.resorts_later += 1,
                }
            }
            if !q.dry_run {
                changed += 1;
            }
            if samples.len() < 20 {
                samples.push(serde_json::json!({
                    "user": user, "thread_id": tid, "shared": shared,
                    "category": [r.category_from, to],
                    "activity": [r.activity_from, r.activity_to],
                }));
            }
        }
    }

    let sum = |f: fn(&UserTally) -> u64| -> u64 { by_user.values().map(f).sum() };
    let would_change = sum(|t| t.change);
    let done = walked < q.limit as u64;
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        would_change,
        changed,
        "thread standing recompute"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "all": q.all,
            "skip": q.skip,
            "limit": q.limit,
            "walked": walked,
            "would_change": would_change,
            "tab_moves": sum(|t| t.tab_moves),
            "resorts": sum(|t| t.resorts),
            "category_unknown": sum(|t| t.category_unknown),
            "requires_action_set": sum(|t| t.requires_action_set),
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
