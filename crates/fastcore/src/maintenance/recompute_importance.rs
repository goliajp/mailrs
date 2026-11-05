//! Put each reader's own importance verdict back on their membership row.
//!
//! The verdict used to be written to the shared thread hash too, and every
//! write that derives a membership row from that hash copied it onto the
//! next member: a conversation two accounts hold showed the second one the
//! first one's score, and an unscored arrival (a reader's own reply) put
//! the other member's score back on the first. Stopping the copy fixes new
//! writes; the rows it already wrote stay wrong until this runs.
//!
//! # What each row should say
//!
//! In order, the first that answers:
//!
//! 1. **The reader's decision log** (`mailrs-threadstate` in their
//!    mailbox). Every verdict scored for them since 2026-08-15 is recorded
//!    there under their name, so it is theirs by construction — and it is
//!    the score as it was given, which a rescore today cannot reproduce
//!    because the contact tallies it reads have moved since.
//! 2. **A rescore of their newest inbound message** in the conversation,
//!    for rows the log does not mention (verdicts older than the log, or
//!    copied from another member). The result is recorded in the log, so a
//!    second run takes it from step 1 and agrees with itself.
//! 3. **Unscored**, when they have no inbound message there whose file can
//!    be read — a conversation of only their own sends, which the ingest
//!    path never scores. Whatever the row holds was somebody else's.
//!
//! Recompute rather than wait for the next message: most conversations get
//! no next message, so a wrong verdict would stay for good.
//!
//! # Batching and convergence
//!
//! `skip` / `limit` walk the (user, conversation) pairs in a fixed order —
//! accounts sorted, each account's conversations sorted — so a long run is
//! a sequence of calls; `next_skip` is where the next one starts. Defaults
//! to a dry run. A row is written only when it differs; a second run over
//! the same pairs reports `changed: 0`.

use std::collections::BTreeMap;

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct RecomputeImportanceQuery {
    /// Restrict to one mailbox. Absent means every account.
    user: Option<String>,
    /// Report what would change without changing it. On unless asked off.
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn yes() -> bool {
    true
}

fn default_limit() -> usize {
    2000
}

/// Where a row's correct verdict came from.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Log,
    Rescored,
    Unscorable,
}

type Verdict = Option<(String, f64)>;

fn same(a: &Verdict, b: &Verdict) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some((la, sa)), Some((lb, sb))) => la == lb && (sa - sb).abs() <= 1e-6,
        _ => false,
    }
}

fn logged(log: &mailrs_threadstate::ThreadState, tid: &str) -> Verdict {
    let rec = log.get(tid)?;
    let level = rec.importance_level.clone()?;
    Some((level, rec.importance_score.unwrap_or(0.0)))
}

#[derive(Default, serde::Serialize)]
struct UserTally {
    walked: u64,
    agree: u64,
    change: u64,
    from_log: u64,
    rescored: u64,
    cleared: u64,
    /// Of `change`: the row holds exactly another member's logged verdict.
    another_members_verdict: u64,
}

/// `POST /v1/admin/maintenance:recompute-thread-importance[?user=&dry_run=false&skip=&limit=]`
pub(crate) async fn recompute_thread_importance_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RecomputeImportanceQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let accounts = match state.mailbox.list_account_addresses() {
        Ok(mut u) => {
            u.sort();
            u
        }
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let users: Vec<String> = match &q.user {
        Some(u) => vec![u.clone()],
        None => accounts.clone(),
    };
    // Every account's log, for "is this another member's verdict".
    let logs: BTreeMap<String, mailrs_threadstate::ThreadState> = accounts
        .iter()
        .chain(users.iter())
        .map(|u| (u.clone(), crate::threadstate::load(u)))
        .collect();

    let mut position = 0usize;
    let mut walked = 0u64;
    let mut agree = 0u64;
    let mut changed = 0u64;
    let mut row_missing = 0u64;
    let mut errors = 0u64;
    let mut log_recorded = 0u64;
    let mut multi_owner_changes = 0u64;
    let mut by_user: BTreeMap<String, UserTally> = BTreeMap::new();
    let end = q.skip.saturating_add(q.limit);

    'users: for user in &users {
        let tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
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
            let Ok(Some(row)) = state.mailbox.get_thread_for_user(user, tid) else {
                row_missing += 1;
                continue;
            };
            let have: Verdict = match row.importance_level.is_empty() {
                true => None,
                false => Some((row.importance_level.clone(), row.importance_score)),
            };
            let (want, source, fresh) = match logged(&logs[user], tid) {
                Some(v) => (Some(v), Source::Log, None),
                None => match crate::importance::rescore(&state, user, tid) {
                    Some((level, score)) => (
                        Some((level.as_str().to_string(), score as f64)),
                        Source::Rescored,
                        Some((level, score)),
                    ),
                    None => (None, Source::Unscorable, None),
                },
            };
            if same(&have, &want) {
                agree += 1;
                tally.agree += 1;
                // A rescore that agrees still goes in the log, so the next
                // run reads it there rather than rescoring a moved input.
                if let Some((level, score)) = fresh
                    && !q.dry_run
                {
                    crate::importance::record_in_log(user, tid, level, score);
                    log_recorded += 1;
                }
                continue;
            }
            tally.change += 1;
            match source {
                Source::Log => tally.from_log += 1,
                Source::Rescored => tally.rescored += 1,
                Source::Unscorable => tally.cleared += 1,
            }
            let others: Vec<&String> = accounts
                .iter()
                .filter(|o| *o != user)
                .filter(|o| matches!(state.mailbox.get_thread_for_user(o, tid), Ok(Some(_))))
                .collect();
            if !others.is_empty() {
                multi_owner_changes += 1;
            }
            if have.is_some()
                && others
                    .iter()
                    .any(|o| same(&logged(&logs[o.as_str()], tid), &have))
            {
                tally.another_members_verdict += 1;
            }
            if q.dry_run {
                continue;
            }
            let wrote = match (source, &want, fresh) {
                (Source::Rescored, _, Some((level, score))) => {
                    // the row and the log together
                    crate::importance::record_verdict(&state, user, tid, level, score);
                    log_recorded += 1;
                    Ok(true)
                }
                (Source::Log, Some((level, score)), _) => state
                    .mailbox
                    .set_thread_importance(user, tid, level, *score)
                    .map(|()| true),
                _ => state.mailbox.clear_thread_importance(user, tid),
            };
            match wrote {
                Ok(true) => changed += 1,
                Ok(false) => {}
                Err(e) => {
                    errors += 1;
                    tracing::warn!(err = %e, %user, %tid, "importance recompute: write failed");
                }
            }
        }
    }

    let would_change: u64 = by_user.values().map(|t| t.change).sum();
    let done = walked < q.limit as u64;
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        agree,
        would_change,
        changed,
        "thread importance recompute"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "skip": q.skip,
            "limit": q.limit,
            // What it looked at, so a zero below reads as agreement rather
            // than as an empty walk.
            "walked": walked,
            "agree": agree,
            "would_change": would_change,
            "changed": changed,
            "row_missing": row_missing,
            "errors": errors,
            "multi_owner_changes": multi_owner_changes,
            "log_recorded": log_recorded,
            "by_user": by_user,
            "done": done,
            "next_skip": q.skip + walked as usize,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
