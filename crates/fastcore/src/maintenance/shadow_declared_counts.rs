//! `POST /v1/admin/maintenance:count-shadow` — what the engine counts,
//! against what the rows say.
//!
//! Stage C2. Read-only, and deliberately so: the aggregate index declared
//! in 2.70.0 is written and read by nothing, and this is what has to agree
//! before that changes.
//!
//! **Read `differs_with_every_row_grouped`, not the totals.** Production's
//! rows predate the group column, so the engine counts short for nearly
//! all of them and the per-field figures will open large. That is the
//! migration debt, and `rules/measure-before-you-cut-over.md` is about
//! exactly this: on 2026-08-02 the same shape opened at 19,779 and
//! converged to 74, and cutting over on the first number would have shown
//! 19,463 conversations the wrong importance in the name of repairing 74.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;

use crate::FastcoreState;

pub(crate) async fn count_shadow_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    // What the store did while this ran — see `store_motion`.
    let motion = crate::store_motion::begin(&state);
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(crate::store_motion::with_motion(
            serde_json::json!({ "users": users }),
            motion.finish(&state),
        ))
        .into_response();
    };
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    let r = match state.mailbox.shadow_declared_counts(user, offset, limit) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(err = %e, %user, "declared-count shadow failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // The global debt, alongside the per-user comparison: a backfill's
    // progress reads off one number rather than thirteen. Only on the first
    // page, because it walks the whole keyspace.
    let debt = match offset {
        0 => state.mailbox.ungrouped_user_message_rows().ok(),
        _ => None,
    };

    if r.differs_with_every_row_grouped > 0 {
        tracing::warn!(
            %user,
            differs = r.differs_with_every_row_grouped,
            "threads whose rows are all grouped and whose counts still disagree"
        );
    }

    let body = serde_json::json!({
        "user": user,
        "offset": offset,
        "limit": limit,
        "scanned": r.scanned,
        "agreed": r.agreed,
        // Per field. Never summed: the three drift for different reasons.
        "count_differs": r.count_differs,
        "unread_differs": r.unread_differs,
        "sent_differs": r.sent_differs,
        // The debt — rows the index cannot see because they predate the
        // column. Expected to open large and to reach zero after C3.
        "threads_with_ungrouped_rows": r.threads_with_ungrouped_rows,
        "ungrouped_rows": r.ungrouped_rows,
        "rows_total": debt.map(|d| d.0),
        "rows_ungrouped_store_wide": debt.map(|d| d.1),
        // The defect. This is the number that gates the read cutover.
        "differs_with_every_row_grouped": r.differs_with_every_row_grouped,
        "samples": r.samples,
        "done": (r.scanned as i64) < limit,
    });
    Json(crate::store_motion::with_motion(
        body,
        motion.finish(&state),
    ))
    .into_response()
}
