//! A user's labels: making, renaming and deleting them, putting them on
//! conversations, the list of one label's conversations, and the label ids
//! every listed row carries.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::label as lb;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox_kevy::LabelError;
use mailrs_mailbox_kevy::{LabelOpError, ListThreadsFilter, ThreadRow};

use crate::FastcoreState;

fn refused(e: LabelOpError) -> Response {
    let status = match &e {
        LabelOpError::Label(LabelError::Taken(_)) => StatusCode::CONFLICT,
        LabelOpError::Label(LabelError::NotFound) => StatusCode::NOT_FOUND,
        LabelOpError::Label(_) => StatusCode::BAD_REQUEST,
        LabelOpError::Io(e) => {
            tracing::warn!(error = %e, "label store failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, e.to_string()).into_response()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// `GET /v1/users/{user}/labels`.
pub(crate) async fn list_labels(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
) -> Response {
    match state.mailbox.labels(&user) {
        Ok(items) => Json(lb::LabelsResponse { items }).into_response(),
        Err(e) => refused(e.into()),
    }
}

/// `POST /v1/users/{user}/labels`.
pub(crate) async fn create_label(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<lb::CreateLabelRequest>,
) -> Response {
    match state.mailbox.create_label(&user, &req.name, &req.color, now()) {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// `POST /v1/users/{user}/labels/{label_id}`.
pub(crate) async fn update_label(
    State(state): State<Arc<FastcoreState>>,
    Path((user, id)): Path<(String, String)>,
    Json(req): Json<lb::UpdateLabelRequest>,
) -> Response {
    let updated = state
        .mailbox
        .update_label(&user, &id, req.name.as_deref(), req.color.as_deref());
    match updated {
        Ok(l) => Json(l).into_response(),
        Err(e) => refused(e),
    }
}

/// `DELETE /v1/users/{user}/labels/{label_id}`.
pub(crate) async fn delete_label(
    State(state): State<Arc<FastcoreState>>,
    Path((user, id)): Path<(String, String)>,
) -> Response {
    match state.mailbox.delete_label(&user, &id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => refused(LabelError::NotFound.into()),
        Err(e) => refused(e.into()),
    }
}

async fn set(state: &FastcoreState, user: &str, tid: &str, id: &str, on: bool) -> Response {
    match state.mailbox.set_label(user, id, tid, on) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refused(e),
    }
}

/// `POST /v1/users/{user}/threads/{thread_id}/labels/{label_id}`.
pub(crate) async fn label_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&state, &user, &tid, &id, true).await
}

/// `DELETE /v1/users/{user}/threads/{thread_id}/labels/{label_id}`.
pub(crate) async fn unlabel_thread(
    State(state): State<Arc<FastcoreState>>,
    Path((user, tid, id)): Path<(String, String, String)>,
) -> Response {
    set(&state, &user, &tid, &id, false).await
}

/// One label's conversations, newest first: read from the label's set and
/// sorted here. Archived ones are in it, as a label is not a place; those
/// in the Trash are not, and held ones only when asked for.
pub(crate) fn rows_with_label(
    state: &FastcoreState,
    user: &str,
    id: &str,
    filter: &ListThreadsFilter<'_>,
    limit: usize,
) -> std::io::Result<Vec<ThreadRow>> {
    let mut rows = Vec::new();
    for tid in state.mailbox.threads_with_label(user, id)? {
        let Some(row) = state.mailbox.get_thread_for_user(user, &tid)? else {
            continue;
        };
        let held = row.quarantined
            && filter.quarantine != mailrs_mailbox_kevy::QuarantineScope::Include;
        let shown = row.trashed_at == 0
            && !held
            && filter.before_ts.is_none_or(|ts| row.latest_date < ts)
            && (!filter.has_unread || row.unread_count > 0)
            && (!filter.starred || row.starred);
        if shown {
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| b.latest_date.cmp(&a.latest_date).then(b.thread_id.cmp(&a.thread_id)));
    rows.truncate(limit);
    Ok(rows)
}

/// `items` with each one's label ids, from one read per label.
pub(crate) fn with_labels(
    state: &FastcoreState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    let by_thread = match state.mailbox.labels_by_thread(user) {
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
