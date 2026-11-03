//! Conversations nobody holds: the operator's list and one-at-a-time
//! delete. The check that nobody holds it is made again at delete time.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::{OwnerlessThreadWire, OwnerlessThreadsResponse};
use mailrs_mailbox_kevy::OwnerlessDelete;

use crate::FastcoreState;

fn wire(t: mailrs_mailbox_kevy::OwnerlessThread) -> OwnerlessThreadWire {
    OwnerlessThreadWire {
        thread_id: t.thread_id,
        messages: t.messages,
        latest_at: t.latest_at,
    }
}

/// `GET /v1/admin/ownerless-threads`
pub(crate) async fn list(State(state): State<Arc<FastcoreState>>) -> Response {
    let found = tokio::task::spawn_blocking(move || state.mailbox.ownerless_threads()).await;
    match found {
        Ok(Ok(items)) => Json(OwnerlessThreadsResponse {
            items: items.into_iter().map(wire).collect(),
        })
        .into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "ownerless threads: listing failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "ownerless threads: listing panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/ownerless-threads/{thread_id}/delete`
pub(crate) async fn delete(
    State(state): State<Arc<FastcoreState>>,
    Path(thread_id): Path<String>,
) -> Response {
    let tid = thread_id.clone();
    let done =
        tokio::task::spawn_blocking(move || state.mailbox.delete_ownerless_thread(&tid)).await;
    match done {
        Ok(Ok(OwnerlessDelete::Deleted(t))) => {
            tracing::warn!(thread_id = %thread_id, messages = t.messages, "ownerless thread deleted");
            Json(wire(t)).into_response()
        }
        Ok(Ok(OwnerlessDelete::Held)) => StatusCode::CONFLICT.into_response(),
        Ok(Ok(OwnerlessDelete::Missing)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "ownerless threads: delete failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "ownerless threads: delete panicked");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
