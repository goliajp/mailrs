//! `POST /v1/users/{user}/sent`: the sender's own copy of a message the
//! web tier queued, filed by this core (`mailrs_intake::sent`).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::sent::{FileSentRequest, FileSentResponse};

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

pub async fn file_sent(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<FileSentRequest>,
) -> Result<Json<FileSentResponse>, StatusCode> {
    let body = req.message().map_err(|_| StatusCode::BAD_REQUEST)?;
    // built here, on a runtime thread: the directory keeps its handle
    let core = PgCore {
        dir: PgDirectory::new(
            state.domain.clone(),
            state.maildir_root.clone(),
            state.net_url.clone(),
        ),
        mailbox: state.mailbox.clone(),
        event_bus: state.event_bus.clone(),
    };
    let filed = tokio::task::spawn_blocking(move || {
        mailrs_intake::sent::file(&core, &user, &body, &req.recipients).map_err(|e| {
            tracing::error!(error = %e, %user, "filing the sent copy failed");
            match e.kind() {
                std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "filing the sent copy panicked");
        StatusCode::INTERNAL_SERVER_ERROR
    })??;
    Ok(Json(FileSentResponse {
        thread_id: filed.thread_id,
        message_id: filed.message_id,
        blob_ref: filed.blob_ref,
    }))
}
