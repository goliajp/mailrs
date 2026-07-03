//! `/api/mail/inline-upload` + `/api/mail/inline/{id}` — store an
//! inline attachment (usually a pasted / dragged image in the rich
//! editor) in network kevy and hand back a URL the compose form can
//! embed into the outgoing HTML.
//!
//! Keys:
//!   inline:<uuid>          hash  { content_type: <mt>, body: <bytes> }
//!
//! `body` is stored raw (not base64) since kevy is byte-safe.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Multipart, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn random_id() -> String {
    let mut bytes = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// POST /api/mail/inline-upload — multipart form with `file` field.
/// Returns `{url: "/api/mail/inline/<id>"}`.
pub async fn inline_upload(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(_user)): Extension<AuthedUser>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Only care about the first file field — the compose form always
    // sends one `file` at a time.
    let field = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .ok_or(StatusCode::BAD_REQUEST)?;
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
    let id = random_id();
    let key = format!("inline:{id}");
    let ct_c = content_type.clone();
    let body_v = bytes.to_vec();
    with_kevy(move |c| {
        c.hset(
            key.as_bytes(),
            &[
                (b"content_type" as &[u8], ct_c.as_bytes()),
                (b"body", body_v.as_slice()),
            ],
        )?;
        Ok(())
    })?;
    Ok(Json(serde_json::json!({
        "url": format!("/api/mail/inline/{id}"),
    })))
}

/// GET /api/mail/inline/{id} — return the stored bytes with their
/// original content-type.
pub async fn get_inline(
    State(_state): State<Arc<WebState>>,
    Path(id): Path<String>,
) -> Result<axum::response::Response, StatusCode> {
    let key = format!("inline:{id}");
    let key_c = key.clone();
    let ct = with_kevy(move |c| c.hget(key_c.as_bytes(), b"content_type"))?;
    let body = with_kevy(move |c| c.hget(key.as_bytes(), b"body"))?;
    let Some(body) = body else {
        return Err(StatusCode::NOT_FOUND);
    };
    let ct = ct
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", ct)
        .header("cache-control", "public, max-age=86400")
        .body(axum::body::Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
