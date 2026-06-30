//! Handlers for `mailrs_core_api::method::analysis`.
//!
//! Phase 2.2 — analysis + attachment text endpoints.
//! `semantic_search` returns `BackendUnsupported` (501) from monolith too —
//! fastcore intentionally; monolith just forwards via meili externally
//! (out of scope for this Phase).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::analysis as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/analysis/{message_id}
pub async fn get_analysis(
    State(state): State<Arc<CoreRpcState>>,
    Path(message_id): Path<i64>,
) -> Result<Json<wire::GetAnalysisResponse>, StatusCode> {
    let row = state
        .mailbox
        .get_email_analysis(message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, message_id, "get_analysis failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let analysis = row.map(|r| wire::EmailAnalysisWire {
        message_id: r.message_id,
        category: r.category,
        risk_score: r.risk_score,
        risk_reason: r.risk_reason,
        summary: r.summary,
        people: r.people,
        dates: r.dates,
        amounts: r.amounts,
        action_items: r.action_items,
        model_version: r.model_version,
        clean_text: r.clean_text,
        requires_action: r.requires_action,
        sender_intent: r.sender_intent,
        action_deadline: r.action_deadline,
    });
    Ok(Json(wire::GetAnalysisResponse { analysis }))
}

/// GET /v1/analysis:unanalyzed-count?model_version=
pub async fn count_unanalyzed(
    State(state): State<Arc<CoreRpcState>>,
    Query(q): Query<wire::ListUnanalyzedQuery>,
) -> Result<Json<wire::CountUnanalyzedResponse>, StatusCode> {
    let count = state
        .mailbox
        .count_unanalyzed_messages(&q.model_version)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "count_unanalyzed failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::CountUnanalyzedResponse { count }))
}

/// POST /v1/analysis/{message_id}/boost-importance
pub async fn boost_importance(
    State(state): State<Arc<CoreRpcState>>,
    Path(message_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .boost_importance_for_action(message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, message_id, "boost_importance failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/messages/{id}/attachments/texts
pub async fn attachment_texts(
    State(state): State<Arc<CoreRpcState>>,
    Path(message_id): Path<i64>,
) -> Result<Json<wire::AttachmentTextsResponse>, StatusCode> {
    let joined = state
        .mailbox
        .get_attachment_texts(message_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, message_id, "attachment_texts failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    // Inherent returns a single joined String. Split if needed by the
    // caller; we return [joined] when non-empty, [] otherwise.
    let texts = if joined.is_empty() {
        Vec::new()
    } else {
        vec![joined]
    };
    Ok(Json(wire::AttachmentTextsResponse { texts }))
}

/// POST /v1/search/semantic
///
/// fastcore returns 501 by design (no pgvector). Monolith semantic_search
/// IS available on pgvector but this Phase 2.2 wire surface returns 501
/// uniformly so webapi has a single consistent behavior to code against.
/// Direct pgvector access remains via the existing web layer for now.
pub async fn semantic_search() -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
