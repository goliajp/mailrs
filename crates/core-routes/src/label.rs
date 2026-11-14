//! A user's labels: making, renaming and deleting them, and putting them on
//! conversations.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::label::{self as lb, LabelError};

use crate::store::{LabelFailure, LabelStore};

fn refused(e: LabelFailure) -> Response {
    let status = match &e {
        LabelFailure::Refused(LabelError::Taken(_)) => StatusCode::CONFLICT,
        LabelFailure::Refused(LabelError::NotFound) => StatusCode::NOT_FOUND,
        LabelFailure::Refused(_) => StatusCode::BAD_REQUEST,
        LabelFailure::Store(e) => {
            tracing::warn!(error = %e, "label store failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, e.to_string()).into_response()
}

/// GET /v1/users/{user}/labels
pub async fn list_labels<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
) -> Response {
    match state.labels(&user).await {
        Ok(items) => Json(lb::LabelsResponse { items }).into_response(),
        Err(e) => refused(e),
    }
}

/// POST /v1/users/{user}/labels
pub async fn create_label<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Json(req): Json<lb::CreateLabelRequest>,
) -> Response {
    let now = chrono::Utc::now().timestamp();
    match state.create_label(&user, &req.name, &req.color, now).await {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// POST /v1/users/{user}/labels/{label_id}
pub async fn update_label<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, String)>,
    Json(req): Json<lb::UpdateLabelRequest>,
) -> Response {
    let updated = state
        .update_label(&user, &id, req.name.as_deref(), req.color.as_deref())
        .await;
    match updated {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// DELETE /v1/users/{user}/labels/{label_id}
pub async fn delete_label<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    match state.delete_label(&user, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(LabelError::NotFound.into()),
        Err(e) => refused(e),
    }
}

async fn set<S: LabelStore>(state: &S, user: &str, tid: &str, id: &str, on: bool) -> Response {
    match state.set_label(user, id, tid, on).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refused(e),
    }
}

/// POST /v1/users/{user}/threads/{thread_id}/labels/{label_id}
pub async fn label_thread<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&*state, &user, &tid, &id, true).await
}

/// DELETE /v1/users/{user}/threads/{thread_id}/labels/{label_id}
pub async fn unlabel_thread<S: LabelStore>(
    State(state): State<Arc<S>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&*state, &user, &tid, &id, false).await
}
