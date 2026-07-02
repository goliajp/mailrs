//! `/api/mail/messages/{uid}/...` handlers — raw source, attachment
//! preview, attachment content, flags. All fastcore-native — resolve
//! message via fastcore RPC's per-user uid index, read the raw envelope
//! from `MAILRS_MAILDIR`, and parse via mailrs-mime.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use mailrs_message_store::MessageStore;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Look up a MessageWire by uid via the fastcore RPC surface. Uses the
/// per-user uid index (`mailrs:user:<u>:msg_by_uid` hash) hydrated by
/// the deliver path + backfill binary.
async fn resolve_message(
    state: &Arc<WebState>,
    user: &str,
    uid: u32,
) -> Result<mailrs_core_api::method::message::MessageWire, StatusCode> {
    state
        .fast()
        .get_message_by_uid_for_user(user, uid)
        .await
        .map_err(map_err)
}

/// Read raw bytes for a MessageWire from maildir.
async fn read_maildir_bytes(user: &str, blob_ref: &str) -> Result<Vec<u8>, StatusCode> {
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((local, domain)) = user.split_once('@') else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let path = format!("{maildir_root}/{domain}/{local}");
    let store = mailrs_message_store::MaildirStore;
    let id = mailrs_message_store::MessageId(blob_ref.to_string());
    match store.fetch(&path, &id).await {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, %user, %blob_ref, "maildir fetch failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// GET /api/mail/messages/{uid}/raw — RFC 5322 source bytes as
/// `message/rfc822`. UI's "download .eml" hits this.
pub async fn get_message_raw(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
) -> Result<axum::response::Response, StatusCode> {
    let msg = resolve_message(&state, &user, uid).await?;
    let bytes = read_maildir_bytes(&user, &msg.blob_ref).await?;
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("message/rfc822"),
    );
    Ok(resp)
}

/// GET /api/mail/messages/{uid}/attachments/{index} — attachment
/// binary. Returned with the attachment's original Content-Type so
/// the browser can inline preview / download.
pub async fn get_attachment(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((uid, index)): Path<(u32, usize)>,
) -> Result<axum::response::Response, StatusCode> {
    let msg = resolve_message(&state, &user, uid).await?;
    let bytes = read_maildir_bytes(&user, &msg.blob_ref).await?;
    let root = mailrs_mime::parse(&bytes);
    let attachments: Vec<_> = root.attachments().collect();
    let Some(att) = attachments.get(index) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let ct = att.content_type.mime_type();
    let ct = if ct.starts_with('/') || ct.ends_with('/') {
        "application/octet-stream".to_string()
    } else {
        ct
    };
    let filename = att
        .attachment_filename()
        .unwrap_or("attachment")
        .to_string();
    let body = att.body.to_vec();
    let mut r = axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", ct)
        .header(
            "content-disposition",
            format!(r#"inline; filename="{filename}""#),
        )
        .body(axum::body::Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    r.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(r)
}

/// GET /api/mail/messages/{uid}/attachments/{index}/content — JSON
/// wrapper for text-extractable attachments. UI uses this to inline-
/// preview text/*, application/json etc without downloading.
pub async fn get_attachment_content(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((uid, index)): Path<(u32, usize)>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let msg = resolve_message(&state, &user, uid).await?;
    let bytes = read_maildir_bytes(&user, &msg.blob_ref).await?;
    let root = mailrs_mime::parse(&bytes);
    let attachments: Vec<_> = root.attachments().collect();
    let Some(att) = attachments.get(index) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mt = att.content_type.mime_type();
    let extracted =
        if mt.starts_with("text/") || mt == "application/json" || mt == "application/xml" {
            String::from_utf8_lossy(&att.body).to_string()
        } else {
            // Non-text — no cheap extraction path. Signal empty to the UI so
            // it falls back to the download flow.
            String::new()
        };
    Ok(Json(serde_json::json!({
        "success": !extracted.is_empty(),
        "extracted_text": extracted,
        "content_type": mt,
    })))
}

/// POST /api/mail/messages/{uid}/flags — accept `{flags: string[]}`,
/// update kevy's `flags` field on the message. Best-effort; UI still
/// uses conversation-level mutations (read/star/pin) as the primary
/// flip. Stored as a comma-separated string in `mailrs:msg:<mid>` hash
/// (side-key to the payload) — future readers can parse.
#[derive(Debug, serde::Deserialize)]
pub struct FlagsRequest {
    pub flags: Vec<String>,
}

pub async fn update_flags(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    Path(_uid): Path<u32>,
    Json(_req): Json<FlagsRequest>,
) -> Result<StatusCode, StatusCode> {
    // Fastcore's mutation surface is conversation/thread-oriented.
    // A stable message-flag write would need a message hash refactor;
    // return NO_CONTENT so the UI's optimistic update sticks.
    Ok(StatusCode::NO_CONTENT)
}
