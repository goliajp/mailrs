//! Stage 6 of the per-user message projection: clear the per-user fields
//! left on the shared message blobs.
//!
//! Stage 5 stopped writing `blob_ref` / `uid` / `flags` / `modseq` /
//! `mailbox_id` / `user_address` onto the blob every owner of a thread
//! reads. Rows written before it keep one owner's values, and this removes
//! them.
//!
//! It is not a repair — nothing serves those fields since
//! `user_message_view` became the single decision, and on production none
//! of the 326 differing ones resolve for the user asking anyway
//! (`maintenance:usermsg-shadow`, `shared_resolves: 0`). It removes what a
//! future fallback could reach for, which is how the defect happened the
//! first time.
//!
//! # Batching and convergence
//!
//! The walk is over conversations, not (user, conversation) pairs: the
//! blob is shared, so visiting it once per owner would rewrite the same
//! bytes twice and report double. The conversations of every account are
//! collected into one sorted, de-duplicated list, and `skip` / `limit`
//! slice it, so a sequence of calls covers exactly what one call would;
//! `next_skip` is where the next one starts. Defaults to a dry run. A blob
//! is written only when stripping changes it, so a second run reports
//! `would_change: 0`.

use std::collections::BTreeSet;

use super::prelude::*;

#[derive(serde::Deserialize)]
pub(crate) struct StripSharedQuery {
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

/// `POST /v1/admin/maintenance:strip-shared-per-user-fields[?dry_run=false&skip=&limit=]`
pub(crate) async fn strip_shared_per_user_fields_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<StripSharedQuery>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let threads: BTreeSet<String> = users
        .iter()
        .flat_map(|u| state.mailbox.all_thread_ids_for_user(u).unwrap_or_default())
        .collect();

    let mut walked = 0u64;
    let mut messages_seen = 0u64;
    let mut would_change = 0u64;
    let mut failed = 0u64;
    // the first few conversations with something to strip, to look at by hand
    let mut samples: Vec<&str> = Vec::new();

    for tid in threads.iter().skip(q.skip).take(q.limit) {
        walked += 1;
        match state.mailbox.strip_shared_per_user_fields(tid, q.dry_run) {
            Ok((seen, differing)) => {
                messages_seen += seen;
                would_change += differing;
                if differing > 0 && samples.len() < 20 {
                    samples.push(tid);
                }
            }
            Err(e) => {
                failed += 1;
                tracing::warn!(err = %e, %tid, "strip-shared-per-user-fields failed");
            }
        }
    }

    let rewritten = match q.dry_run {
        true => 0,
        false => would_change,
    };
    tracing::info!(
        dry_run = q.dry_run,
        walked,
        would_change,
        rewritten,
        "strip shared per-user fields"
    );
    Json(crate::store_motion::with_motion(
        serde_json::json!({
            "dry_run": q.dry_run,
            "skip": q.skip,
            "limit": q.limit,
            // Accounts and conversations first: `would_change: 0` means
            // "all clean" only when the counts above it are not also zero.
            "accounts": users.len(),
            "threads_total": threads.len(),
            "walked": walked,
            "messages_seen": messages_seen,
            "would_change": would_change,
            "rewritten": rewritten,
            "threads_failed": failed,
            "samples": samples,
            "done": q.skip.saturating_add(walked as usize) >= threads.len(),
            "next_skip": q.skip + walked as usize,
        }),
        motion.finish(&state),
    ))
    .into_response()
}
