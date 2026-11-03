//! `/api/mail/inline-upload` + `/api/mail/inline/{id}` — store an
//! inline attachment (usually a pasted / dragged image in the rich
//! editor) in network kevy and hand back a URL the compose form can
//! embed into the outgoing HTML.
//!
//! Keys:
//!
//! ```text
//! inline:<id>                 hash    { content_type, body, owner }, expires
//! inline-uploaded:<user>      string  bytes uploaded this window, expires
//! ```
//!
//! `body` is stored raw (not base64) since kevy is byte-safe. An upload
//! is needed only until the message that shows it is sent, when it
//! travels inside the message under a Content-ID, so it expires after
//! [`KEEP`]. Each user may upload [`MAX_BYTES_PER_WINDOW`] in a window of
//! that length, which bounds what one account can leave in the store.

use std::sync::Arc;

use crate::handlers::kevy_util::with_kevy;
use axum::{
    Json,
    extract::{Extension, Multipart, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn random_id() -> String {
    let mut bytes = [0u8; 16];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// How long an upload is kept: a draft that shows it can wait this long.
pub(crate) const KEEP: std::time::Duration = std::time::Duration::from_secs(30 * 86_400);

/// What one user may upload within one [`KEEP`].
pub(crate) const MAX_BYTES_PER_WINDOW: i64 = 200 * 1024 * 1024;

fn uploaded_key(user: &str) -> String {
    format!("inline-uploaded:{}", user.to_lowercase())
}

/// Count `len` bytes against `user`'s window. `false` when that would go
/// past [`MAX_BYTES_PER_WINDOW`]; the bytes are then not counted.
fn count_upload(c: &mut kevy_client::Connection, user: &str, len: i64) -> std::io::Result<bool> {
    let key = uploaded_key(user);
    let total = c.incr_by(key.as_bytes(), len)?;
    if total == len {
        c.expire(key.as_bytes(), KEEP)?;
    }
    if total > MAX_BYTES_PER_WINDOW {
        c.incr_by(key.as_bytes(), -len)?;
        return Ok(false);
    }
    Ok(true)
}

/// POST /api/mail/inline-upload — multipart form with `file` field.
/// Returns `{url: "/api/mail/inline/<id>"}`, or 429 when the user has
/// uploaded [`MAX_BYTES_PER_WINDOW`] already.
pub async fn inline_upload(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
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
        .to_ascii_lowercase();
    // served from this origin to whoever opens the link, so only image
    // types a browser draws without running anything; SVG can carry script
    if !is_inert_image(&content_type) {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
    let id = random_id();
    let key = format!("inline:{id}");
    let ct_c = content_type.clone();
    let body_v = bytes.to_vec();
    let counted = with_kevy(move |c| {
        if !count_upload(c, &user, body_v.len() as i64)? {
            return Ok(false);
        }
        c.hset(
            key.as_bytes(),
            &[
                (b"content_type" as &[u8], ct_c.as_bytes()),
                (b"body", body_v.as_slice()),
                (b"owner", user.as_bytes()),
            ],
        )?;
        c.expire(key.as_bytes(), KEEP)?;
        Ok(true)
    })?;
    if !counted {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
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
    let ct = with_kevy(move |c| {
        c.hget(key_c.as_bytes(), b"content_type")
            .map_err(std::io::Error::from)
    })?;
    let body = with_kevy(move |c| {
        c.hget(key.as_bytes(), b"body")
            .map_err(std::io::Error::from)
    })?;
    let Some(body) = body else {
        return Err(StatusCode::NOT_FOUND);
    };
    let ct = ct
        .map(|b| String::from_utf8_lossy(&b).to_ascii_lowercase())
        .unwrap_or_else(|| "application/octet-stream".to_string());
    // An upload from before the type check may be anything; it downloads
    // instead of rendering here, and nothing is sniffed or scripted.
    let (ct, disposition) = match is_inert_image(&ct) {
        true => (ct, "inline"),
        false => ("application/octet-stream".to_string(), "attachment"),
    };
    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("content-type", ct)
        .header("content-disposition", disposition)
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", "sandbox")
        .header("cache-control", "public, max-age=86400")
        .body(axum::body::Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

const UPLOAD_PATH: &str = "/api/mail/inline/";

/// The upload id an `<img src>` value names, when it is one of ours: a
/// relative or absolute URL ending in `/api/mail/inline/<32 hex>`.
fn upload_id(src: &str) -> Option<&str> {
    let (_, id) = src.rsplit_once(UPLOAD_PATH)?;
    match id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Some(id),
        false => None,
    }
}

/// Every `src` value in `html` that names one of our uploads, in order,
/// with the byte range of the value.
fn upload_srcs(html: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = html[at..].find("src=") {
        let open = at + found + 4;
        at = open;
        let Some(quote) = html[open..]
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
        else {
            continue;
        };
        let start = open + 1;
        let Some(len) = html[start..].find(quote) else {
            break;
        };
        let value = start..start + len;
        if let Some(id) = upload_id(&html[value.clone()]) {
            out.push((value.clone(), id));
        }
        at = value.end;
    }
    out
}

/// The Content-ID an upload travels under in a sent message.
fn upload_cid(id: &str) -> String {
    format!("{id}@inline")
}

/// `html` with each `src` naming an upload in `found` pointed at its
/// Content-ID instead. A src naming an upload that is not in `found` is
/// left as it was.
fn point_at_cids(html: &str, found: &std::collections::HashSet<String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut copied = 0;
    for (range, id) in upload_srcs(html) {
        if !found.contains(id) {
            continue;
        }
        out.push_str(&html[copied..range.start]);
        out.push_str("cid:");
        out.push_str(&upload_cid(id));
        copied = range.end;
    }
    out.push_str(&html[copied..]);
    out
}

/// Carry the images the user pasted into this message inside it.
///
/// The editor shows a pasted image from this server's upload URL, which
/// nobody outside can open, so a recipient elsewhere saw a broken image.
/// Each upload the HTML shows becomes a part of the message under a
/// Content-ID, and the HTML points at that instead. An upload that is
/// gone, or is not an image, keeps its URL.
pub(crate) async fn embed_uploads(parts: &mut crate::handlers::compose::ComposeParts) {
    let ids: Vec<String> = upload_srcs(&parts.html_body)
        .into_iter()
        .map(|(_, id)| id.to_string())
        .collect();
    let mut found = std::collections::HashSet::new();
    for id in ids {
        if found.contains(&id) {
            continue;
        }
        let key = format!("inline:{id}");
        let stored = with_kevy(move |c| c.hgetall(key.as_bytes()).map_err(std::io::Error::from));
        let Ok(flat) = stored else {
            tracing::warn!("send: a pasted image could not be read; its link is kept");
            continue;
        };
        let mut content_type = String::new();
        let mut body = Vec::new();
        for pair in flat.chunks(2) {
            if let [k, v] = pair {
                match k.as_slice() {
                    b"content_type" => {
                        content_type = String::from_utf8_lossy(v).to_ascii_lowercase()
                    }
                    b"body" => body = v.clone(),
                    _ => {}
                }
            }
        }
        if body.is_empty() || !is_inert_image(&content_type) {
            continue;
        }
        let ext = content_type.trim_start_matches("image/").to_string();
        parts
            .attachments
            .push(crate::handlers::compose::Attachment {
                filename: format!("image-{}.{ext}", &id[..8]),
                content_type,
                bytes: body,
                content_id: Some(upload_cid(&id)),
            });
        found.insert(id);
    }
    if !found.is_empty() {
        parts.html_body = point_at_cids(&parts.html_body, &found);
    }
}

fn is_inert_image(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/avif"
    )
}

#[cfg(test)]
mod tests {
    use super::{is_inert_image, point_at_cids, upload_srcs};

    const A: &str = "0123456789abcdef0123456789abcdef";
    const B: &str = "fedcba9876543210fedcba9876543210";

    #[test]
    fn only_our_upload_links_are_pointed_at_their_content_ids() {
        let html = format!(
            "<p><img src=\"/api/mail/inline/{A}\"> <img src='https://mail.example.test/api/mail/inline/{B}'> \
             <img src=\"https://cdn.example.test/logo.png\"> <img src=\"/api/mail/inline/short\"></p>"
        );
        let ids: Vec<&str> = upload_srcs(&html).into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids, [A, B]);
        let found = [A.to_string()].into_iter().collect();
        let out = point_at_cids(&html, &found);
        assert!(out.contains(&format!("src=\"cid:{A}@inline\"")), "{out}");
        assert!(
            out.contains(&format!("/api/mail/inline/{B}'")),
            "one not found keeps its link: {out}"
        );
        assert!(out.contains("https://cdn.example.test/logo.png"), "{out}");
    }

    #[test]
    fn only_images_that_cannot_run_script_are_kept() {
        for ok in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
            assert!(is_inert_image(ok), "{ok}");
        }
        for bad in [
            "image/svg+xml",
            "text/html",
            "application/xhtml+xml",
            "application/octet-stream",
        ] {
            assert!(!is_inert_image(bad), "{bad}");
        }
    }
}
