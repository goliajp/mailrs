//! `POST /v1/admin/maintenance:backfill-ledger` — records mail delivered
//! before the fact ledger existed, so that a change of reputation can find
//! it too.
//!
//! Inbound mail only, the same as delivery: a reader's own messages are not
//! facts about a sender. Facts are written once, so a message already in the
//! ledger costs one lookup and nothing is rewritten; a second run over the
//! same mail writes nothing. Dry run by default; paged by `skip` / `limit`.

use super::hangup::{Hangup, run_blocking};
use super::prelude::*;

const PAUSE_EVERY: u64 = 100;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct BackfillQuery {
    /// Report without writing. **Default true.**
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
    5_000
}
fn default_pause_ms() -> u64 {
    20
}

pub(crate) async fn backfill_ledger_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<BackfillQuery>,
) -> axum::response::Response {
    match run_blocking(move |h| walk(&state, &q, h)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "backfill-ledger: listing accounts failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "backfill-ledger task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Default)]
struct Counts {
    seen: u64,
    walked: u64,
    already: u64,
    own: u64,
    no_file: u64,
    recorded: u64,
}

fn walk(
    state: &Arc<FastcoreState>,
    q: &BackfillQuery,
    hangup: &Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut c = Counts::default();
    let mut stopped_early = false;
    'walk: for user in &users {
        let on_disk = crate::maildir_scan::paths_on_disk(state, user);
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                c.seen += 1;
                if c.seen <= q.skip {
                    continue;
                }
                if c.walked >= q.limit {
                    stopped_early = true;
                    break 'walk;
                }
                if hangup.hung_up() {
                    tracing::warn!(
                        walked = c.walked,
                        "backfill-ledger: the caller hung up, stopping"
                    );
                    stopped_early = true;
                    break 'walk;
                }
                c.walked += 1;
                if c.walked.is_multiple_of(PAUSE_EVERY) {
                    std::thread::sleep(std::time::Duration::from_millis(q.pause_ms));
                }
                if state.mailbox.ledger_has(user, &mid)? {
                    c.already += 1;
                    continue;
                }
                let Some(raw) = state
                    .mailbox
                    .user_message_facts(user, &mid)
                    .ok()
                    .flatten()
                    .and_then(|copy| {
                        on_disk
                            .get(crate::maildir_scan::base_id(&copy.blob_ref))
                            .cloned()
                    })
                    .and_then(|path| std::fs::read(path).ok())
                else {
                    c.no_file += 1;
                    continue;
                };
                if crate::ledger::is_own(user, &raw) {
                    c.own += 1;
                    continue;
                }
                if q.dry_run {
                    c.recorded += 1;
                    continue;
                }
                if crate::ledger::record_delivery(state, user, &mid, &raw) {
                    c.recorded += 1;
                } else {
                    c.already += 1;
                }
            }
        }
    }
    tracing::info!(
        walked = c.walked,
        already = c.already,
        own = c.own,
        no_file = c.no_file,
        recorded = c.recorded,
        dry_run = q.dry_run,
        "backfill-ledger"
    );
    Ok(serde_json::json!({
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "messages_walked": c.walked,
        "already_recorded": c.already,
        "own_mail": c.own,
        "no_file": c.no_file,
        "recorded": c.recorded,
        "next_skip": q.skip + c.walked,
        "done": !stopped_early,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::hangup::{Hangup, seed};
    use super::*;

    #[test]
    fn a_walk_stops_soon_after_the_caller_hangs_up() {
        let state = crate::tests::fresh_state();
        seed(&state, "u@x", 5);
        let q = BackfillQuery {
            dry_run: true,
            skip: 0,
            limit: 100,
            pause_ms: 0,
        };
        let all = walk(&state, &q, &Hangup::never()).unwrap();
        assert_eq!(all["messages_walked"], 5, "{all}");
        // seeded rows have no file behind them
        assert_eq!(all["no_file"], 5, "{all}");
        let cut = walk(&state, &q, &Hangup::after(2)).unwrap();
        assert_eq!(cut["messages_walked"], 2, "{cut}");
        assert_eq!(cut["done"], false, "{cut}");
    }
}
