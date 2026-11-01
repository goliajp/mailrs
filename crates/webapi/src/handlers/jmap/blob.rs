//! Uploads and downloads (RFC 8620 §6).
//!
//! An upload is kept under `MAILRS_JMAP_UPLOAD_DIR` (`/data/jmap-uploads`),
//! one directory per account, named by its SHA-256: the same bytes
//! uploaded twice by one account are one file, and accounts never share.
//! Each account may keep [`ACCOUNT_LIMIT`] bytes; a file nobody has used
//! for [`KEEP_FOR`] is deleted by [`spawn_sweep`].
//!
//! Every download is an attachment with `nosniff` and a sandboxing CSP,
//! whatever the client asks for: a message part is somebody else's HTML.
//! A blob of another account, or another account named, is not found.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use axum::extract::{Extension, Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::ctx::Ctx;
use super::email::leaves;
use super::ids::{self, Blob};
use super::limits::{self, UploadSlot};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// What one account may keep uploaded.
pub const ACCOUNT_LIMIT: u64 = 1024 * 1024 * 1024;
/// How long an unused upload is kept. RFC 8620 §6.1 asks for an hour at
/// least.
pub const KEEP_FOR: Duration = Duration::from_secs(24 * 3600);

pub fn root() -> PathBuf {
    std::env::var("MAILRS_JMAP_UPLOAD_DIR")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .map_or_else(|| PathBuf::from("/data/jmap-uploads"), PathBuf::from)
}

/// An account's directory: a hash of the address, so no address is a
/// path.
fn account_dir(root: &Path, user: &str) -> PathBuf {
    let h = Sha256::digest(user.trim().to_ascii_lowercase().as_bytes());
    root.join(hex::encode(&h[..16]))
}

fn used(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

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
    let sha = hex::encode(Sha256::digest(&body));
    let dir = account_dir(&root(), &user);
    let path = dir.join(&sha);
    let tmp = dir.join(format!(".{sha}.part"));
    let size = body.len();
    let stored = tokio::task::spawn_blocking(move || -> std::io::Result<Result<(), ()>> {
        std::fs::create_dir_all(&dir)?;
        if path.exists() {
            // used again: kept another day
            std::fs::File::options()
                .append(true)
                .open(&path)?
                .set_modified(SystemTime::now())?;
            return Ok(Ok(()));
        }
        if used(&dir) + size as u64 > ACCOUNT_LIMIT {
            return Ok(Err(()));
        }
        std::fs::write(&tmp, &body)?;
        std::fs::rename(&tmp, &path)?;
        Ok(Ok(()))
    })
    .await;
    match stored {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(()))) => {
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
    }
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
            let _ = tokio::task::spawn_blocking(move || {
                std::fs::File::options()
                    .append(true)
                    .open(&path)
                    .and_then(|f| f.set_modified(SystemTime::now()))
            })
            .await;
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

/// A message's file. The view decides what is the caller's: a uid is
/// resolved in the caller's account only.
async fn message(ctx: &Ctx, uid: u32) -> Option<Vec<u8>> {
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
fn media_type(asked: Option<&str>) -> String {
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
fn disposition(name: &str) -> String {
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

/// One pass of the sweep: delete uploads unused for [`KEEP_FOR`], and the
/// account directories left empty. With nothing uploaded it lists one
/// directory. Returns how many files it deleted.
pub fn sweep(root: &Path, now: SystemTime) -> usize {
    let Ok(accounts) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut deleted = 0;
    for account in accounts.filter_map(Result::ok) {
        let dir = account.path();
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut left = 0;
        for f in files.filter_map(Result::ok) {
            let old = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age >= KEEP_FOR);
            match old && std::fs::remove_file(f.path()).is_ok() {
                true => deleted += 1,
                false => left += 1,
            }
        }
        if left == 0 {
            let _ = std::fs::remove_dir(&dir);
        }
    }
    deleted
}

/// Sweep every hour.
pub fn spawn_sweep() {
    tokio::spawn(async {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            let deleted = tokio::task::spawn_blocking(|| sweep(&root(), SystemTime::now()))
                .await
                .unwrap_or(0);
            if deleted > 0 {
                tracing::info!(deleted, "jmap uploads: unused for a day, deleted");
            }
        }
    });
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
pub fn health() -> serde_json::Value {
    let root = root();
    let bytes: u64 = std::fs::read_dir(&root)
        .map(|it| it.filter_map(Result::ok).map(|d| used(&d.path())).sum())
        .unwrap_or(0);
    let at = match root.exists() {
        true => root.as_path(),
        false => root.parent().unwrap_or(Path::new("/")),
    };
    json!({ "bytes": bytes, "volume_free_bytes": free_bytes(at) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_deletes_what_went_unused_for_a_day() {
        let root = tempfile::tempdir().unwrap();
        let a = account_dir(root.path(), "a@x.test");
        let b = account_dir(root.path(), "b@x.test");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("old"), b"x").unwrap();
        std::fs::write(b.join("new"), b"y").unwrap();
        let day_ago = SystemTime::now() - KEEP_FOR - Duration::from_secs(5);
        std::fs::File::options()
            .append(true)
            .open(a.join("old"))
            .unwrap()
            .set_modified(day_ago)
            .unwrap();
        assert_eq!(sweep(root.path(), SystemTime::now()), 1);
        assert!(!a.exists(), "an emptied account directory goes");
        assert!(b.join("new").exists());
        assert_eq!(
            sweep(root.path(), SystemTime::now()),
            0,
            "nothing more to do"
        );
        assert_ne!(account_dir(root.path(), "A@x.test "), b);
        assert_eq!(account_dir(root.path(), "A@x.test "), a);
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
