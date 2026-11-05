//! Uploads and downloads (RFC 8620 §6).
//!
//! An upload goes into the upload store drafts share
//! (`mailrs_core_sidestate::families::uploads`): per account, by SHA-256,
//! up to its `ACCOUNT_LIMIT`, deleted a day after last use unless a draft
//! names it.
//!
//! Every download is an attachment with `nosniff` and a sandboxing CSP,
//! whatever the client asks for: a message part is somebody else's HTML.
//! A blob of another account, or another account named, is not found.

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use axum::body::Bytes;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::ctx::Ctx;
use super::email::leaves;
use super::ids::{self, Blob};
use super::limits::{self, UploadSlot};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

use mailrs_core_sidestate::families::uploads::{self, Put, account_dir, root};

fn problem(status: StatusCode, detail: &str, limit: Option<&str>) -> Response {
    let mut body = json!({ "type": limits::LIMIT, "status": status.as_u16(), "detail": detail });
    if let Some(l) = limit {
        body["limit"] = json!(l);
    }
    (
        status,
        [(header::CONTENT_TYPE, "application/problem+json")],
        body.to_string(),
    )
        .into_response()
}

/// POST /jmap/upload/{accountId}/
pub async fn upload(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    UrlPath(account): UrlPath<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    if !account.eq_ignore_ascii_case(&user) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if body.len() > limits::MAX_SIZE_UPLOAD {
        return problem(
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload too large",
            Some("maxSizeUpload"),
        );
    }
    let Some(_slot) = UploadSlot::take(&user) else {
        return problem(
            StatusCode::TOO_MANY_REQUESTS,
            "too many uploads at once",
            Some("maxConcurrentUpload"),
        );
    };
    let size = body.len();
    let user_c = user.clone();
    let stored = tokio::task::spawn_blocking(move || {
        uploads::put(&root(), &user_c, &body, SystemTime::now())
    })
    .await;
    let sha = match stored {
        Ok(Ok(Put::Stored(sha))) => sha,
        Ok(Ok(Put::Full)) => {
            return problem(
                StatusCode::INSUFFICIENT_STORAGE,
                "this account's uploads are full; they are deleted a day after last use",
                None,
            );
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, %user, "jmap upload: not stored");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let kind = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    (
        StatusCode::CREATED,
        axum::Json(json!({
            "accountId": user,
            "blobId": ids::blob(&Blob::Upload(sha)),
            "type": kind,
            "size": size,
        })),
    )
        .into_response()
}

/// The bytes a blob id names, if it is the caller's.
pub async fn read(ctx: &Ctx, blob_id: &str) -> Option<Vec<u8>> {
    match ids::parse_blob(blob_id)? {
        Blob::Upload(sha) => {
            let path = account_dir(&root(), &ctx.user).join(&sha);
            let bytes = tokio::fs::read(&path).await.ok()?;
            // used: kept another day
            let _ =
                tokio::task::spawn_blocking(move || uploads::touch(&path, SystemTime::now())).await;
            Some(bytes)
        }
        Blob::Message(uid) => message(ctx, uid).await,
        Blob::Part(uid, n) => {
            let raw = message(ctx, uid).await?;
            let root = mailrs_mime::parse(&raw);
            let parts = leaves(&root);
            parts.get(n).map(|p| p.body.to_vec())
        }
    }
}

/// A message's file, when the message is one the caller's mailboxes show:
/// a uid is resolved in the caller's account only, and a message the view
/// leaves out (held as suspected fraud) is no more there for a blob than
/// for `Email/get`. Whatever the reason, the answer is the same `None`.
async fn message(ctx: &Ctx, uid: u32) -> Option<Vec<u8>> {
    let view = super::view::current(ctx).await.ok()?;
    view.get(uid)?;
    let w = ctx
        .state
        .core
        .get_message_by_uid_for_user(&ctx.user, uid)
        .await
        .ok()?;
    super::email::raw_of(&ctx.user, &w).await
}

#[derive(Deserialize)]
pub struct DownloadParams {
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

/// A media type made only of token characters, or `application/octet-stream`.
pub(crate) fn media_type(asked: Option<&str>) -> String {
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
    };
    match asked.and_then(|t| t.split_once('/')) {
        Some((a, b)) if token(a) && token(b) => format!("{a}/{b}").to_ascii_lowercase(),
        _ => "application/octet-stream".into(),
    }
}

/// `attachment; filename*=UTF-8''…`, the name percent-encoded.
pub(crate) fn disposition(name: &str) -> String {
    let mut enc = String::new();
    for b in name.bytes() {
        match b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            true => enc.push(b as char),
            false => enc.push_str(&format!("%{b:02X}")),
        }
    }
    format!("attachment; filename*=UTF-8''{enc}")
}

/// GET /jmap/download/{accountId}/{blobId}/{name}
pub async fn download(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    UrlPath((account, blob_id, name)): UrlPath<(String, String, String)>,
    Query(params): Query<DownloadParams>,
) -> Response {
    if !account.eq_ignore_ascii_case(&user) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let ctx = Ctx::new(state, user, None);
    let Some(bytes) = read(&ctx, &blob_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, media_type(params.kind.as_deref())),
            (header::CONTENT_DISPOSITION, disposition(&name)),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CONTENT_SECURITY_POLICY, "sandbox".to_string()),
            (
                header::CACHE_CONTROL,
                "private, immutable, max-age=31536000".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

/// Bytes available to an unprivileged writer under `path`.
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a writable statvfs
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

/// For /api/health: what the uploads hold and what the volume has left.
/// The volume is measured at the directory, or at its parent before the
/// first upload makes it.
pub fn health(expired_unreferenced: serde_json::Value) -> serde_json::Value {
    let root = root();
    let at = match root.exists() {
        true => root.as_path(),
        false => root.parent().unwrap_or(Path::new("/")),
    };
    json!({
        "bytes": uploads::total_bytes(&root),
        "volume_free_bytes": free_bytes(at),
        "expired_unreferenced": expired_unreferenced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jmap_upload_id_is_the_one_a_draft_names() {
        let sha = "ab".repeat(32);
        let id = ids::blob(&Blob::Upload(sha.clone()));
        assert_eq!(id, uploads::blob_id(&sha));
        assert_eq!(uploads::sha_of(&id), Some(sha));
    }

    #[test]
    fn downloads_name_a_safe_type_and_an_encoded_file() {
        assert_eq!(media_type(Some("text/html")), "text/html");
        assert_eq!(
            media_type(Some("text/html\r\nx: y")),
            "application/octet-stream"
        );
        assert_eq!(media_type(None), "application/octet-stream");
        assert_eq!(
            disposition("a b\"\r\n.pdf"),
            "attachment; filename*=UTF-8''a%20b%22%0D%0A.pdf"
        );
    }
}
