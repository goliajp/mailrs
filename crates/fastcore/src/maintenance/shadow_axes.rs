//! `POST /v1/admin/maintenance:axis-shadow` — the two declared columns
//! that decide which list a thread appears in, against the engine.
//!
//! Stage C4, read-only. `sent_only` gates the Inbox ORDERPATH and
//! `is_sender` keys the Sent axis, so a wrong correction here does not
//! show a wrong number — it empties a folder. Measured first.
//!
//! **`sent_only_differs_shared` is the number to read.** The defect is a
//! cross-user leak: `thread_user_pairs` derives both columns from the
//! `ThreadRow` its caller holds, and `set_thread_date` hands it the
//! **shared** hash, whose counters are everybody's. That can only bite a
//! thread with more than one owner. A difference on a single-owner thread
//! means the *correction* is wrong, not the column, and it has to be
//! understood before anything is written.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;

use crate::FastcoreState;

pub(crate) async fn axis_shadow_route(
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
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    let r = match state.mailbox.shadow_axis_columns(user, offset, limit) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(err = %e, %user, "axis shadow failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if r.sent_only_differs > r.sent_only_differs_shared {
        tracing::warn!(
            %user,
            single_owner = r.sent_only_differs - r.sent_only_differs_shared,
            "sent_only disagrees on threads with one owner — the correction, not the column"
        );
    }

    let body = serde_json::json!({
        "user": user,
        "offset": offset,
        "limit": limit,
        "scanned": r.scanned,
        "shared": r.shared,
        // Where the defect can live.
        "sent_only_differs_shared": r.sent_only_differs_shared,
        "is_sender_differs_shared": r.is_sender_differs_shared,
        // And outside it, which would mean the derivation is wrong.
        "sent_only_differs": r.sent_only_differs,
        "is_sender_differs": r.is_sender_differs,
        // Threads the index cannot answer for: counted apart, never as
        // agreement.
        "not_indexed": r.not_indexed,
        "samples": r.samples,
        "done": (r.scanned as i64) < limit,
    });
    Json(crate::store_motion::with_motion(
        body,
        motion.finish(&state),
    ))
    .into_response()
}
