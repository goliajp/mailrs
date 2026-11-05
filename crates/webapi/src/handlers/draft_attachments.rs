//! `/api/mail/draft-attachments` — a file for a draft goes into the upload
//! store the moment it is added, and the draft names it by `blob_id`.
//!
//! The upload answers the `DraftAttachment` the draft save takes; the
//! download serves the caller's own uploads only, as an attachment with
//! `nosniff` and a sandboxing CSP, like a JMAP download.

use std::time::SystemTime;

use axum::Json;
use axum::extract::{Extension, Multipart, Path, Query};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use mailrs_core_api::method::admin::DraftAttachment;
use mailrs_core_sidestate::families::uploads::{self, MAX_ATTACHMENT_BYTES, Put};
use serde::Deserialize;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::jmap::{UploadSlot, disposition, media_type};
use crate::handlers::send_error::SendError;

/// POST /api/mail/draft-attachments — multipart, one file field.
pub async fn upload(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    mut form: Multipart,
) -> Result<Json<DraftAttachment>, SendError> {
    let field = form
        .next_field()
        .await
        .map_err(SendError::form)?
        .ok_or_else(|| SendError::new(StatusCode::BAD_REQUEST, "no file in the form"))?;
    let name = field.file_name().unwrap_or("attachment").to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = field.bytes().await.map_err(SendError::form)?;
    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(SendError::too_large());
    }
    let Some(_slot) = UploadSlot::take(&user) else {
        return Err(SendError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too many uploads at once",
        ));
    };
    let size = bytes.len() as u64;
    let who = user.clone();
    let stored = tokio::task::spawn_blocking(move || {
        uploads::put(&uploads::root(), &who, &bytes, SystemTime::now())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match stored {
        Ok(Put::Stored(sha)) => Ok(Json(DraftAttachment {
            blob_id: uploads::blob_id(&sha),
            name,
            content_type,
            size,
        })),
        Ok(Put::Full) => Err(SendError::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "this account's uploads are full; files no draft keeps are deleted a day after last use",
        )),
        Err(e) => {
            tracing::error!(error = %e, %user, "draft attachment: not stored");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into())
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Download {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

/// GET /api/mail/draft-attachments/{blob_id}?name=&type= — the caller's
/// own upload; anyone else's, or none, is not found.
pub async fn download(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(blob_id): Path<String>,
    Query(q): Query<Download>,
) -> Response {
    let Some(path) = uploads::path_of(&uploads::root(), &user, &blob_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let name = q.name.unwrap_or_else(|| "attachment".into());
    (
        [
            (header::CONTENT_TYPE, media_type(q.kind.as_deref())),
            (header::CONTENT_DISPOSITION, disposition(&name)),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
            (header::CACHE_CONTROL, "private, max-age=0".to_string()),
        ],
        bytes,
    )
        .into_response()
}
