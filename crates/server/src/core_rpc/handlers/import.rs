//! `POST /v1/users/{user}/import`: a message brought over from another
//! mailbox, filed by this core (`mailrs_intake::import`).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::import::ImportMessageRequest;
use mailrs_core_api::method::sent::FileSentResponse;

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

pub async fn import_message(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<ImportMessageRequest>,
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
        mailrs_intake::import::file(&core, &user, &body).map_err(|e| {
            tracing::error!(error = %e, %user, "importing a message failed");
            match e.kind() {
                std::io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        })
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "importing a message panicked");
        StatusCode::INTERNAL_SERVER_ERROR
    })??;
    Ok(Json(FileSentResponse {
        thread_id: filed.thread_id,
        message_id: filed.message_id,
        blob_ref: filed.blob_ref,
    }))
}
