//! A user's labels, as fastcore keeps them: making, renaming and deleting,
//! putting them on conversations, and the label ids on listed rows.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::label as lb;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox::labels::LabelError;
use mailrs_mailbox::pg::LabelDbError;

use super::thread::require_member;
use crate::core_rpc::CoreRpcState;

fn refused(e: LabelDbError) -> Response {
    let status = match &e {
        LabelDbError::Label(LabelError::Taken(_)) => StatusCode::CONFLICT,
        LabelDbError::Label(LabelError::NotFound) => StatusCode::NOT_FOUND,
        LabelDbError::Label(_) => StatusCode::BAD_REQUEST,
        LabelDbError::Db(e) => {
            tracing::warn!(error = %e, "label store failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, e.to_string()).into_response()
}

/// GET /v1/users/{user}/labels
pub async fn list_labels(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Response {
    match state.mailbox.labels(&user).await {
        Ok(items) => Json(lb::LabelsResponse { items }).into_response(),
        Err(e) => refused(e.into()),
    }
}

/// POST /v1/users/{user}/labels
pub async fn create_label(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<lb::CreateLabelRequest>,
) -> Response {
    let now = chrono::Utc::now().timestamp();
    match state
        .mailbox
        .create_label(&user, &req.name, &req.color, now)
        .await
    {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// POST /v1/users/{user}/labels/{label_id}
pub async fn update_label(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, String)>,
    Json(req): Json<lb::UpdateLabelRequest>,
) -> Response {
    let updated = state
        .mailbox
        .update_label(&user, &id, req.name.as_deref(), req.color.as_deref())
        .await;
    match updated {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// DELETE /v1/users/{user}/labels/{label_id}
pub async fn delete_label(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    match state.mailbox.delete_label(&user, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(LabelError::NotFound.into()),
        Err(e) => refused(e),
    }
}

async fn set(state: &CoreRpcState, user: &str, tid: &str, id: &str, on: bool) -> Response {
    if let Err(status) = require_member(state, user, tid).await {
        return status.into_response();
    }
    match state.mailbox.set_label(user, id, tid, on).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refused(e),
    }
}

/// POST /v1/users/{user}/threads/{thread_id}/labels/{label_id}
pub async fn label_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&state, &user, &tid, &id, true).await
}

/// DELETE /v1/users/{user}/threads/{thread_id}/labels/{label_id}
pub async fn unlabel_thread(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&state, &user, &tid, &id, false).await
}

/// `items` with each one's label ids.
pub(crate) async fn with_labels(
    state: &CoreRpcState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    let by_thread = match state.mailbox.labels_by_thread(user).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(%user, error = %e, "labels not read; rows shown without them");
            return items;
        }
    };
    for item in &mut items {
        if let Some(ids) = by_thread.get(&item.thread_id) {
            item.labels = ids.clone();
        }
    }
    items
}
