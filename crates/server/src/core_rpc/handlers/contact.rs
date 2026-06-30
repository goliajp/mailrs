//! Handlers for `mailrs_core_api::method::contact`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use mailrs_core_api::method::contact as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/contacts:search?q=&limit=
pub async fn search_contacts(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Query(q): Query<wire::SearchContactsQuery>,
) -> Result<Json<wire::SearchContactsResponse>, StatusCode> {
    let items = state
        .mailbox
        .search_contacts(&user, &q.q, q.limit)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "search_contacts failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::SearchContactsResponse { items }))
}

/// POST /v1/users/{user}/contacts/{email}:inbound
pub async fn upsert_inbound(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, email)): Path<(String, String)>,
    Json(req): Json<wire::UpsertInboundContactRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .upsert_contact_inbound(
            &user,
            &email,
            &req.display_name,
            req.is_mailing_list,
            req.is_automated,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, email = %email, "upsert inbound failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/users/{user}/contacts/{email}/scoring
pub async fn contact_scoring(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, email)): Path<(String, String)>,
) -> Result<Json<wire::ContactScoring>, StatusCode> {
    let info_opt = state
        .mailbox
        .get_contact_for_scoring(&user, &email)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, email = %email, "scoring failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let scoring = match info_opt {
        Some(info) => wire::ContactScoring {
            is_mutual: info.is_mutual,
            is_mailing_list: info.is_mailing_list,
            is_vip: info.is_vip,
            is_blocked: info.is_blocked,
            importance_bias: info.importance_bias,
            received_count: info.received_count.max(0) as u32,
            sent_count: info.sent_count.max(0) as u32,
        },
        None => wire::ContactScoring::default(),
    };
    Ok(Json(scoring))
}

/// POST /v1/users/{user}/contacts/{email}/feedback
pub async fn sender_feedback(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, email)): Path<(String, String)>,
    Json(req): Json<wire::SenderFeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .mailbox
        .record_sender_feedback(&user, &email, &req.action)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, email = %email, "sender_feedback failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /v1/users/{user}/contacts/{email}:has-sent-to
pub async fn has_sent_to(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, email)): Path<(String, String)>,
) -> Result<Json<wire::HasSentToResponse>, StatusCode> {
    let has_sent = state
        .mailbox
        .has_sent_to(&user, &email)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, email = %email, "has_sent_to failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::HasSentToResponse { has_sent }))
}
