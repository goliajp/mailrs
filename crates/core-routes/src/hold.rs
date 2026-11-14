//! Letting go of a held conversation, and what was let go before.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::thread as wire;

use crate::store::HoldStore;

/// POST /v1/users/{user}/quarantine/{thread_id}/release — it was not
/// fraud. The conversation goes back to whatever list it belonged to; the
/// verdict on the message is left alone, since it records what the rules
/// saw then. One not held is a member's no-op; one not theirs is a 404.
pub async fn release<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Response {
    let fail = |e: crate::store::StoreError| {
        tracing::error!(error = %e, %user, %thread_id, "release failed");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    };
    let held = match state.held(&user, &thread_id).await {
        Ok(Some(held)) => held,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return fail(e),
    };
    if held {
        if let Err(e) = state.release_by_person(&user, &thread_id).await {
            return fail(e);
        }
        // Logged rather than merely done: a false positive is the one
        // failure this feature can have, and a release nobody can count is
        // a failure rate nobody can measure.
        tracing::info!(%user, %thread_id, "released: a person said this was not fraud");
        state.queue_images(&user, &thread_id).await;
    }
    Json(wire::ThreadActionResponse {
        affected: 1,
        new_modseq: 0,
    })
    .into_response()
}

/// GET /v1/users/{user}/threads/{thread_id}/releases
pub async fn thread_releases<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Response {
    match state.releases(&user, &thread_id).await {
        Ok(items) => Json(wire::ReleasesResponse { items }).into_response(),
        Err(e) => {
            tracing::error!(error = %e, %user, %thread_id, "reading the releases failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(serde::Deserialize)]
pub struct HeldQuery {
    limit: Option<usize>,
    before_ts: Option<i64>,
    /// The cursor of the last row of the previous page.
    cursor: Option<String>,
}

/// GET /v1/users/{user}/quarantine — the held conversations, newest first,
/// and how many there are. A limit of 0 asks only for the count. A page is
/// at most 1000 rows, above the most ever held (439 on 2026-08-31), so the
/// bound guards against arbitrary work without truncating; `total` says
/// when it bites. A store that cannot be read is a 500: an empty list would
/// read as "nothing held".
pub async fn list_held<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Query(q): Query<HeldQuery>,
) -> Response {
    use mailrs_core_api::paging::Paging;
    let limit = q.limit.unwrap_or(50).min(1000);
    let paged = mailrs_core_api::types::ConversationFilter {
        before_ts: q.before_ts,
        after_cursor: q.cursor,
        ..Default::default()
    };
    let page = match paged.paging(limit) {
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        Ok(Paging::BeforeTs(ts)) => state.held_page(&user, Some(ts), limit).await,
        Ok(Paging::Cursor(mut pager)) => loop {
            let ask = pager.ask();
            match state.held_page(&user, ask.before_ts, ask.limit).await {
                Err(e) => break Err(e),
                Ok((rows, total)) => {
                    if let Some(page) = pager.offer(rows, |r| r.last_date, |r| r.thread_id.as_str())
                    {
                        break Ok((page, total));
                    }
                }
            }
        },
    };
    match page {
        Ok((mut items, total)) => {
            paged.stamp_cursors(&mut items);
            Json(wire::QuarantineListResponse { items, total }).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, %user, "quarantine list failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// GET /v1/users/{user}/messages/{message_id}/fraud-verdict — the stored
/// verdict rather than a re-derivation, since the screen shows the rules
/// in force then. `null` for a message nobody suspected, which is not the
/// same as one examined and cleared; another account's message is a 404.
pub async fn fraud_verdict<S: HoldStore>(
    State(state): State<Arc<S>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Response {
    match state.fraud_verdict(&user, &message_id).await {
        Ok(Some(verdict)) => Json(serde_json::json!({ "verdict": verdict })).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!(error = %e, %message_id, "reading the verdict failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
