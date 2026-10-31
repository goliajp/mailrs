//! The signed-in user's labels: making, renaming, recolouring and deleting
//! them, and putting them on conversations. A name or colour the rules
//! refuse is answered 400 with the reason; a taken name 409.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::error::CoreApiError;
use mailrs_core_api::method::label::{CreateLabelRequest, UpdateLabelRequest};
use mailrs_core_api::method::label::{LabelError, label_color, label_name};

use crate::WebState;
use crate::handlers::conversations::{AuthedUser, map_err};

fn refused(e: LabelError) -> Response {
    let status = match e {
        LabelError::Taken(_) => StatusCode::CONFLICT,
        LabelError::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
}

fn core_refused(e: CoreApiError, name: Option<&str>) -> Response {
    match (e, name) {
        (CoreApiError::Conflict(_), Some(n)) => refused(LabelError::Taken(n.to_string())),
        (CoreApiError::NotFound(_), _) => refused(LabelError::NotFound),
        (e, _) => map_err(e).into_response(),
    }
}

/// GET /api/labels — by name.
pub async fn list_labels(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Response {
    match state.core.list_labels(&user).await {
        Ok(items) => Json(items).into_response(),
        Err(e) => core_refused(e, None),
    }
}

/// POST /api/labels `{name, color}`.
pub async fn create_label(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<CreateLabelRequest>,
) -> Response {
    let checked = label_name(&req.name).and_then(|n| Ok((n, label_color(&req.color)?)));
    let (name, color) = match checked {
        Ok(v) => v,
        Err(e) => return refused(e),
    };
    let req = CreateLabelRequest { name, color };
    match state.core.create_label(&user, &req).await {
        Ok(l) => (StatusCode::CREATED, Json(l)).into_response(),
        Err(e) => core_refused(e, Some(&req.name)),
    }
}

/// POST /api/labels/{id} `{name?, color?}` — rename or recolour.
pub async fn update_label(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
    Json(req): Json<UpdateLabelRequest>,
) -> Response {
    let name = match req.name.as_deref().map(label_name).transpose() {
        Ok(n) => n,
        Err(e) => return refused(e),
    };
    let color = match req.color.as_deref().map(label_color).transpose() {
        Ok(c) => c,
        Err(e) => return refused(e),
    };
    let req = UpdateLabelRequest { name, color };
    match state.core.update_label(&user, &id, &req).await {
        Ok(l) => Json(l).into_response(),
        Err(e) => core_refused(e, req.name.as_deref()),
    }
}

/// DELETE /api/labels/{id} — the conversations stay; only the label goes.
pub async fn delete_label(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Response {
    match state.core.delete_label(&user, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_refused(e, None),
    }
}

/// POST /api/conversations/{thread_id}/labels/{id}
pub async fn label_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((tid, id)): Path<(String, String)>,
) -> Response {
    match state.core.set_thread_label(&user, &tid, &id, true).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_refused(e, None),
    }
}

/// DELETE /api/conversations/{thread_id}/labels/{id}
pub async fn unlabel_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((tid, id)): Path<(String, String)>,
) -> Response {
    match state.core.set_thread_label(&user, &tid, &id, false).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => core_refused(e, None),
    }
}
