//! Conversations held for review, their release, and the fraud verdict a
//! message arrived with.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox::pg::{Held, ThreadQuery};

use crate::core_rpc::CoreRpcState;

#[derive(serde::Deserialize)]
pub struct QuarantineQuery {
    limit: Option<usize>,
    before_ts: Option<i64>,
}

/// GET /v1/users/{user}/quarantine — the held conversations, newest first,
/// and how many there are. A limit of 0 asks only for the count; a page
/// is at most 1000 rows, and `total` says when that bites.
pub async fn list_quarantined(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Query(q): Query<QuarantineQuery>,
) -> Result<Json<wire::QuarantineListResponse>, StatusCode> {
    let query = ThreadQuery {
        held: Held::Only,
        before_ts: q.before_ts,
        ..ThreadQuery::all(&user)
    };
    let rows = state.mailbox.thread_rows(&query).await.map_err(|e| {
        tracing::warn!(error = %e, %user, "quarantine list failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let total = rows.len();
    let limit = q.limit.unwrap_or(50).min(1000);
    let items: Vec<ConversationSummaryWire> = rows.iter().take(limit).map(Into::into).collect();
    let items = super::labels::with_labels(&state, &user, items).await;
    Ok(Json(wire::QuarantineListResponse { items, total }))
}

/// POST /v1/users/{user}/quarantine/{thread_id}/release — a person said it
/// was not fraud. The verdict stays as it was decided.
pub async fn release_quarantined(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Result<Json<wire::ThreadActionResponse>, StatusCode> {
    super::thread::require_member(&state, &user, &thread_id).await?;
    let released = state
        .mailbox
        .release_thread(&user, &thread_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %user, %thread_id, "release failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if released > 0 {
        tracing::info!(%user, %thread_id, "released: a person said this was not fraud");
        // held mail had no images fetched on arrival
        if let Some(url) = state.net_url.clone() {
            match state
                .mailbox
                .thread_raw_messages(&user, &thread_id, &state.maildir_root)
                .await
            {
                Ok(raws) => {
                    let q = mailrs_intake::image_prefetch::NetworkQueue { url };
                    mailrs_intake::image_prefetch::queue_thread(&q, &user, &thread_id, &raws);
                }
                Err(e) => tracing::warn!(error = %e, %user, %thread_id, "images not queued"),
            }
        }
    }
    Ok(Json(wire::ThreadActionResponse {
        affected: released as u32,
        new_modseq: 0,
    }))
}

/// GET /v1/users/{user}/messages/{message_id}/fraud-verdict — `null` for a
/// message nobody suspected, which is not the same as one examined and
/// cleared. Another account's message is a 404: the verdict is stored by
/// Message-ID, not per recipient.
pub async fn fraud_verdict(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, message_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .mailbox
        .find_message_by_message_id(&user, &message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %message_id, "reading the message's owner failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    let verdict = state
        .mailbox
        .fraud_verdict(&message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %message_id, "reading the verdict failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(serde_json::json!({ "verdict": verdict })))
}

/// GET /v1/admin/recipients/{address}/deliverable — whether this core would
/// take mail for the address. A store that cannot be read is a 500, which
/// the receiver treats as "accept": refusing a real address because the
/// database was away is the failure this check must not have.
pub async fn recipient_deliverable(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<mailrs_core_api::method::admin::RecipientDeliverableResponse>, StatusCode> {
    let fwd = address.trim().to_ascii_lowercase();
    if mailrs_core_sidestate::families::mock::is_mock(&fwd) {
        return Ok(Json(
            mailrs_core_api::method::admin::RecipientDeliverableResponse { deliverable: true },
        ));
    }
    let dir = Arc::new(crate::intake::PgDirectory::new(
        state.domain.clone(),
        state.maildir_root.clone(),
        state.net_url.clone(),
    ));
    let route = dir.route(address).await.map_err(|e| {
        tracing::warn!(error = %e, "recipient check: a store could not be read");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let deliverable = !matches!(route, mailrs_intake::route::Route::Nobody);
    Ok(Json(
        mailrs_core_api::method::admin::RecipientDeliverableResponse { deliverable },
    ))
}
