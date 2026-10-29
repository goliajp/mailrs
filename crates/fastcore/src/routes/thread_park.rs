//! Thread verbs that set state for later rather than act now: marking a
//! thread unread, and snoozing it.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use mailrs_core_api::method::thread as th;

use crate::*;

pub(crate) async fn mark_unread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    match state.mailbox.mark_unread(&user, &thread_id) {
        Ok(true) => {}
        Ok(false) => return crate::routes::thread_actions::member_result(false),
        Err(e) => {
            tracing::warn!(error = %e, %user, %thread_id, "mark_unread io error — treating as noop");
        }
    }
    match state.mailbox.mark_newest_message_unseen(&user, &thread_id) {
        Ok(Some((blob_ref, flags))) if !blob_ref.is_empty() => {
            if let Err(e) = crate::maildir_scan::apply_flag_bitmask(&user, &blob_ref, flags) {
                tracing::warn!(error = %e, %user, %blob_ref, "mark unread: rename failed");
            }
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, %user, %thread_id, "mark unread: row sync failed"),
    }
    action_result(true)
}

pub(crate) async fn snooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
    Json(req): Json<th::SnoozeRequest>,
) -> axum::response::Response {
    if let Err(e) = state
        .mailbox
        .set_snoozed(&user, &thread_id, req.snoozed_until)
    {
        tracing::warn!(error = %e, %user, %thread_id, "snooze io error");
    }
    // Tier 1: a snooze carries a timestamp, so no flag or keyword bit can
    // hold it — it goes in the log beside the mail, where a rebuilt index
    // can read it back.
    let mut rec = crate::threadstate::about(&thread_id);
    rec.snoozed_until = Some(req.snoozed_until);
    crate::threadstate::record(&user, &rec);
    axum::http::StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn unsnooze_thread_route(
    State(state): State<Arc<FastcoreState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> axum::response::Response {
    if let Err(e) = state.mailbox.set_snoozed(&user, &thread_id, 0) {
        tracing::warn!(error = %e, %user, %thread_id, "unsnooze io error");
    }
    // Zero is a value here, not an absence: it un-snoozes, and a replay
    // that could not tell it from "nothing said" would put the thread
    // away again.
    let mut rec = crate::threadstate::about(&thread_id);
    rec.snoozed_until = Some(0);
    crate::threadstate::record(&user, &rec);
    axum::http::StatusCode::NO_CONTENT.into_response()
}
