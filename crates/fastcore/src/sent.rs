//! `POST /v1/users/{user}/sent`: the sender's own copy of a message the
//! web tier queued, filed by this core (`mailrs_intake::sent`).

use std::io;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::sent::{FileSentRequest, FileSentResponse};
use mailrs_intake::arrival::{Arrival, Threads};
use mailrs_intake::sent::SentStore;

use crate::FastcoreState;

struct Store(Arc<FastcoreState>);

impl Threads for Store {
    fn thread_for_message_id(&self, user: &str, message_id: &str) -> Option<String> {
        crate::headers::Threads(&self.0).thread_for_message_id(user, message_id)
    }
    fn thread_subject(&self, user: &str, thread_id: &str) -> Option<String> {
        crate::headers::Threads(&self.0).thread_subject(user, thread_id)
    }
}

impl SentStore for Store {
    fn maildir_root(&self) -> &str {
        &self.0.maildir_root
    }
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        self.0.net_conn()
    }
    /// Written as the drain writes an arrival the user wrote; a write this
    /// store cannot make is logged there, and the maildir self-heal indexes
    /// the file later, as for any delivered file.
    fn index_own(
        &self,
        user: &str,
        blob_ref: &str,
        body: &[u8],
        a: &Arrival<'_>,
    ) -> io::Result<()> {
        crate::ingest::store_arrival(&self.0, user, blob_ref, body, a, None);
        Ok(())
    }
}

pub(crate) async fn file_sent(
    State(state): State<Arc<FastcoreState>>,
    Path(user): Path<String>,
    Json(req): Json<FileSentRequest>,
) -> Result<Json<FileSentResponse>, StatusCode> {
    let body = req.message().map_err(|_| StatusCode::BAD_REQUEST)?;
    let store = Store(state);
    let filed = tokio::task::spawn_blocking(move || {
        mailrs_intake::sent::file(&store, &user, &body, &req.recipients).map_err(|e| {
            tracing::error!(error = %e, %user, "filing the sent copy failed");
            match e.kind() {
                io::ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
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
