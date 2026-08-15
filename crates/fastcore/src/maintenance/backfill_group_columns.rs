//! `POST /v1/admin/maintenance:group-backfill` — write the group column
//! onto rows that predate it.
//!
//! Stage C3. Every per-user message row on production was written before
//! the column existed, so the declared index cannot see any of them and
//! `maintenance:count-shadow` opens with the engine counting short for
//! every thread. This closes that, and only then does the shadow's
//! `differs_with_every_row_grouped` mean anything: until a thread's rows
//! are all grouped, it cannot report a defect even if it has one.
//!
//! It reports **what it walked**, not only what it changed — a response
//! that says `written: 0` cannot distinguish "nothing needed doing" from
//! "nothing was there to do", and the second is how a backfill that
//! silently found no input gets read as success.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;

use crate::FastcoreState;

pub(crate) async fn group_backfill_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let motion = crate::store_motion::begin(&state);
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(crate::store_motion::with_motion(
            serde_json::json!({ "users": users }),
            motion.finish(&state),
        ))
        .into_response();
    };
    let dry_run = q.get("dry_run").map(|v| v == "true").unwrap_or(false);

    let tids = match state.mailbox.all_thread_ids_for_user(user) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(err = %e, %user, "group backfill could not list threads");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut threads = 0u64;
    let mut rows_seen = 0u64;
    let mut rows_written = 0u64;
    let mut failed = 0u64;
    for tid in &tids {
        threads += 1;
        let r = match dry_run {
            true => state.mailbox.count_group_columns_to_write(user, tid),
            false => state.mailbox.backfill_group_columns(user, tid),
        };
        match r {
            Ok((seen, written)) => {
                rows_seen += seen;
                rows_written += written;
            }
            Err(e) => {
                tracing::warn!(err = %e, %user, %tid, "group backfill failed for a thread");
                failed += 1;
            }
        }
    }

    if rows_written > 0 {
        tracing::info!(%user, rows_written, threads, "group columns backfilled");
    }

    let body = serde_json::json!({
        "user": user,
        "dry_run": dry_run,
        // What it walked. A `rows_written: 0` beside `threads: 0` is a
        // backfill that found no input; beside `rows_seen: 32000` it is a
        // mailbox that was already converged. The two read identically
        // without these.
        "threads": threads,
        "rows_seen": rows_seen,
        "rows_written": rows_written,
        "failed": failed,
    });
    Json(crate::store_motion::with_motion(
        body,
        motion.finish(&state),
    ))
    .into_response()
}
