//! Marking many conversations read in one call: the whole mailbox, or
//! everything one list shows.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::routes::thread_actions::mark_thread_read_everywhere;
use crate::*;

/// POST `/v1/users/{user}/conversations:mark-all-read` — sweep every
/// unread thread and flip it to seen in one call. UI's "Mark all as
/// read" button was previously batching only the loaded pagination
/// slice, so users with 99+ unread across pages left the tail
/// untouched.
pub(crate) async fn mark_all_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Json<serde_json::Value> {
    // Looped here rather than through `mark_all_seen`, which is the same
    // loop inside the store — and the store cannot rename a file. Held
    // conversations are left as they are: they are not in the badge this
    // clears, and one released later must come back as unread as it was.
    let unread = state
        .mailbox
        .list_unread_not_held(&user)
        .unwrap_or_default();
    let mut flipped = 0u32;
    for tid in &unread {
        if mark_thread_read_everywhere(&state, &user, tid) {
            flipped += 1;
        }
    }
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}

/// Mark read everything **this list** is showing.
///
/// The mailbox-wide version above is the wrong answer when somebody is
/// looking at one list: "mark all as read" from inside Notifications
/// should not silence the inbox, nor from inside Pinned, a label or the
/// Trash the folder around it. The rows are read by [`list_rows`] — the
/// list's own reading, which its search shares — or as Review reads
/// them, so the set marked and the set on screen cannot be different
/// sets.
///
/// Not paged: the point is the whole list, including the rows the
/// client has not scrolled to. A client marking only its loaded page
/// would look finished and do a fraction, which is the failure this
/// route exists to avoid.
///
/// [`list_rows`]: crate::routes::list_rows
pub(crate) async fn mark_list_read_route(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<mailrs_core_api::method::thread::MarkListReadRequest>,
) -> Json<serde_json::Value> {
    // 100_000 is the same ceiling `mark_all_seen` uses
    const WHOLE: usize = 100_000;
    let rows = match req.quarantined {
        true => {
            let filter = ListThreadsFilter {
                quarantine: QuarantineScope::Only,
                ..Default::default()
            };
            state
                .mailbox
                .list_threads_by_activity(&user, &filter, 0, WHOLE)
                .map(|(rows, _)| rows)
        }
        false => crate::routes::list_rows(&state, &user, &req.whole_list(), WHOLE),
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(%user, error = %e, "mark-list-read: list failed");
            return Json(serde_json::json!({ "ok": false, "flipped": 0 }));
        }
    };
    let mut flipped = 0u32;
    for row in &rows {
        // Only the ones that were unread: `mark_seen` answers whether
        // the row existed, not whether anything changed, and a count
        // that includes already-read threads would report work it did
        // not do.
        if row.unread_count > 0 && mark_thread_read_everywhere(&state, &user, &row.thread_id) {
            flipped += 1;
        }
    }
    Json(serde_json::json!({ "ok": true, "flipped": flipped }))
}
