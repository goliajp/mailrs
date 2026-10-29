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
pub(crate) async fn resolve_message(
    state: &Arc<WebState>,
    user: &str,
    uid: u32,
) -> Result<mailrs_core_api::method::message::MessageWire, StatusCode> {
    state
        .core
        .get_message_by_uid_for_user(user, uid)
        .await
        .map_err(map_err)
}

/// Resolve a wire `blob_ref` to the maildir path + bare filename.
///
/// Handles Maildir++ subfolders: self-heal / IMAP APPEND store blob_ref
/// as `<subfolder>/<filename>` for files under `.Sent/`, `.Drafts/`,
/// etc; INBOX files stay bare. The subfolder segment must extend the
/// maildir path — passing the prefixed ref as a filename makes
/// `MaildirStore::fetch` look for it inside INBOX and 404 (that broke
/// attachment preview / raw download for every sent-folder message).
pub(crate) fn blob_ref_location(
    maildir_root: &str,
    user: &str,
    blob_ref: &str,
) -> Option<(String, mailrs_message_store::MessageId)> {
    let (local, domain) = user.split_once('@')?;
    let base = format!("{maildir_root}/{domain}/{local}");
    // The reference convention lives in the maildir stone, which is also
    // what fastcore's reader now uses. It was implemented here and again
    // there, differently, so the same reference resolved in one crate and
    // not the other.
    let (dir, id) = mailrs_message_store::locate(&base, blob_ref)?;
    Some((dir.path().to_string_lossy().into_owned(), id))
}

/// Read raw bytes for a MessageWire from maildir.
/// Shared with prefs.rs for the forward-inline path — a compose-time
/// forward looks up the original .eml the same way `/api/mail/messages/{uid}/raw`
/// does.
pub(crate) async fn read_maildir_bytes_pub(
    user: &str,
    blob_ref: &str,
) -> Result<Vec<u8>, StatusCode> {
    read_maildir_bytes(user, blob_ref).await
}

async fn read_maildir_bytes(user: &str, blob_ref: &str) -> Result<Vec<u8>, StatusCode> {
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let Some((path, id)) = blob_ref_location(&maildir_root, user, blob_ref) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let store = mailrs_message_store::MaildirStore;
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
    let attachments: Vec<_> = root.files().collect();
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
    // The type is whatever the sender wrote, and this is our origin: an
    // HTML or SVG part opened here would run the sender's script with the
    // reader's session. Only types a browser renders without script open
    // in place; everything else downloads, and nothing is sniffed.
    let inline = renders_inert(&ct);
    let mut r = axum::response::Response::builder()
        .status(StatusCode::OK)
        // RFC 6266. A header value must be ASCII, and this filename has
        // just been decoded — the message that prompted the decode was
        // Japanese, so interpolating it raw would make `body()` return
        // Err and the download answer 500. The ASCII `filename=` stays
        // for readers that ignore the extended form.
        .header(
            "content-disposition",
            content_disposition(&filename, inline),
        )
        .header("x-content-type-options", "nosniff")
        .body(axum::body::Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let headers = r.headers_mut();
    headers.insert(
        "cache-control",
        axum::http::HeaderValue::from_static("private, max-age=3600"),
    );
    // Chrome will not show a PDF in a sandboxed document, so a PDF keeps
    // its viewer; its scripts run in the viewer, not on this origin.
    if ct != "application/pdf" {
        headers.insert(
            "content-security-policy",
            axum::http::HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'"),
        );
    }
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_str(&ct).unwrap_or(axum::http::HeaderValue::from_static(
            "application/octet-stream",
        )),
    );
    Ok(r)
}

/// Types a browser shows without running anything the sender wrote. SVG
/// is an image to the eye and a script host to the browser, so it is not
/// one of them.
fn renders_inert(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    matches!(
        ct.as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/avif"
            | "application/pdf"
            | "text/plain"
    ) || ct.starts_with("audio/")
        || ct.starts_with("video/")
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
    let attachments: Vec<_> = root.files().collect();
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

/// POST /api/mail/messages/{uid}/flags — set the message's `flags`
/// bitmask and reconcile thread-level `has_unread` if `\Seen` toggled.
///
/// Wire shape: `{ flags: ["\\Seen", "\\Flagged", ...] }`. The values
/// map to the `mailrs_mailbox::types::FLAG_*` bit constants; anything
/// unrecognised is silently dropped (per RFC 3501 §2.3.2 §5.1.1 for
/// custom `$Label`-style flags future MUAs may send).
///
/// Implementation:
///   1. resolve message via fastcore uid index → `MessageWire`
///   2. patch `wire.flags` to the new bitmask
///   3. HSET the `mailrs:msg:<mid>` blob with the updated JSON
///   4. if `\Seen` changed: bump thread's `unread_count` and reconcile
///      the `user_threads_has_unread` zset via `mark_seen` / `mark_unread`
#[derive(Debug, serde::Deserialize)]
pub struct FlagsRequest {
    pub flags: Vec<String>,
}

fn flag_string_to_bits(labels: &[String]) -> u32 {
    let mut bits: u32 = 0;
    for l in labels {
        match l.as_str() {
            "\\Seen" | "\\seen" | "Seen" => bits |= 0b0000_0001,
            "\\Answered" | "\\answered" | "Answered" => bits |= 0b0000_0010,
            "\\Flagged" | "\\flagged" | "Flagged" => bits |= 0b0000_0100,
            "\\Deleted" | "\\deleted" | "Deleted" => bits |= 0b0000_1000,
            "\\Draft" | "\\draft" | "Draft" => bits |= 0b0001_0000,
            "\\Recent" | "\\recent" | "Recent" => bits |= 0b0010_0000,
            _ => { /* silently drop custom / unknown labels */ }
        }
    }
    bits
}

/// DELETE /api/mail/messages/{uid} — mark the message deleted. In the
/// fastcore model the row lives in the thread's message zset; setting
/// `\Deleted` on the wire's flags bitmask is enough for the UI to
/// hide it. The maildir file is retained; a subsequent expunge (not
/// yet exposed) removes it from disk.
pub async fn delete_message(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Set \Deleted on the wire blob via fastcore so the change lands
    // in the embedded kevy the read path reads. OR-ing FLAG_DELETED
    // preserves other bits (e.g. \Seen) that may already be set.
    let wire = resolve_message(&state, &user, uid).await?;
    let new_flags = wire.flags | 0b0000_1000;
    let set_req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags: new_flags };
    state
        .core
        .set_message_flags(&user, uid, &set_req)
        .await
        .map_err(map_err)?;
    Ok(Json(serde_json::json!({"success": true, "message": null})))
}

/// DELETE /api/mail/pending/{message_id} — the "Undo" after a send.
///
/// The send is found by its own record, so only its owner can stop it,
/// and its jobs are taken out of the queue only while they still wait
/// (`outbound::drop_queued_jobs`). 204 when it was stopped, 404 when
/// there is no such send, 409 when it is already on its way: the client
/// shows that answer, so it must not be "cancelled" when it was not.
pub async fn cancel_pending_send(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    use mailrs_core_sidestate::families::{outbound, send as sendfam, send_read};
    let u = user.clone();
    let s = message_id.clone();
    let item = crate::handlers::kevy_util::with_kevy(move |c| send_read::read_one(c, &u, &s))?
        .ok_or(StatusCode::NOT_FOUND)?;
    if !matches!(
        item.status,
        sendfam::Status::Sending | sendfam::Status::Scheduled
    ) {
        return Err(StatusCode::CONFLICT);
    }
    let s = message_id.clone();
    let u = user.clone();
    let removed =
        crate::handlers::kevy_util::with_kevy(move |c| Ok(outbound::drop_queued_jobs(c, &s, &u)))?;
    if removed.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    let u = user.clone();
    let s = message_id.clone();
    let (cancelled, delivered) = crate::handlers::kevy_util::with_kevy(move |c| {
        sendfam::cancel_send(c, &u, &s, outbound::CANCELLED_REASON)
    })?;
    tracing::info!(%user, %message_id, jobs = removed.len(), cancelled, delivered, "send undone");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_flags(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    Json(req): Json<FlagsRequest>,
) -> Result<StatusCode, StatusCode> {
    let bits = flag_string_to_bits(&req.flags);
    let set_req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags: bits };
    // Fastcore owns the embedded kevy where message blobs live, and it
    // also handles the has_unread zset reconciliation when \Seen
    // toggles. Delegating is what makes the write actually stick.
    state
        .core
        .set_message_flags(&user, uid, &set_req)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── G13.3 · scheduled cancel / reschedule ─────────────────────────

/// `inline; filename="…"; filename*=UTF-8''…` per RFC 6266 §4.1.
///
/// The extended parameter comes from `mailrs_rfc2231::encode_param`,
/// which is the encoding RFC 6266 borrows and was already in the tree
/// for the *sending* side — a second copy here would be a second thing
/// to get wrong. The ASCII `filename=` is the fallback for readers that
/// ignore the extended form; every byte a header cannot carry, and the
/// quote that would end the parameter early, becomes `_`.
fn content_disposition(filename: &str, inline: bool) -> String {
    let kind = if inline { "inline" } else { "attachment" };
    let ascii: String = filename
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() && c != '"' && c != '\\') || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = mailrs_rfc2231::encode_param("filename", filename);
    if filename.is_ascii() {
        // `encode_param` already produced the quoted, escaped form.
        return format!("{kind}; {encoded}");
    }
    format!("{kind}; filename=\"{ascii}\"; {encoded}")
}

#[cfg(test)]
mod tests {
    use super::blob_ref_location;
    use super::content_disposition;
    use super::renders_inert;

    #[test]
    fn bare_filename_maps_to_inbox() {
        let (path, id) = blob_ref_location("/data/maildir", "lihao@golia.jp", "123.M1P2.host")
            .expect("valid user");
        assert_eq!(path, "/data/maildir/golia.jp/lihao");
        assert_eq!(id.0, "123.M1P2.host");
    }

    #[test]
    fn sent_subfolder_extends_path() {
        let (path, id) =
            blob_ref_location("/data/maildir", "lihao@golia.jp", ".Sent/123.M1P2.host")
                .expect("valid user");
        assert_eq!(path, "/data/maildir/golia.jp/lihao/.Sent");
        assert_eq!(id.0, "123.M1P2.host");
    }

    #[test]
    fn non_dot_prefix_stays_bare_id() {
        // a filename containing '/' without a dot-folder prefix is not a
        // Maildir++ subfolder — treat the whole ref as the id
        let (path, id) = blob_ref_location("/data/maildir", "a@b.c", "kevy:msgid").expect("valid");
        assert_eq!(path, "/data/maildir/b.c/a");
        assert_eq!(id.0, "kevy:msgid");
    }

    #[test]
    fn malformed_user_is_none() {
        assert!(blob_ref_location("/data/maildir", "no-at-sign", "x").is_none());
    }

    /// The filename from the 2026-08-03 message. Interpolated raw it
    /// makes an invalid header value, and the attachment 500s.
    #[test]
    fn a_japanese_filename_survives_as_an_extended_parameter() {
        let h = content_disposition("源泉徴収票(2025年分。ZHANG FAN様).pdf", true);
        assert!(
            h.contains("filename*=UTF-8''%E6%BA%90%E6%B3%89"),
            "percent-encoded UTF-8 carries the real name: {h}"
        );
        assert!(h.is_ascii(), "a header value has to be ascii: {h}");
        assert!(
            h.contains(r#"filename="_____(2025___ZHANG FAN_).pdf""#),
            "and the fallback stays readable: {h}"
        );
    }

    /// A quote in a filename would otherwise end the quoted-string and
    /// let the rest of the name be read as further header parameters.
    #[test]
    fn a_quote_cannot_escape_the_fallback_parameter() {
        let h = content_disposition(r#"a"; evil="x.pdf"#, true);
        assert_eq!(
            h, r#"inline; filename="a\"; evil=\"x.pdf""#,
            "the quotes are escaped, so there is still exactly one parameter"
        );
    }

    /// A sender picks the type. Whatever can carry script must download,
    /// whatever a browser shows inertly may open in place.
    #[test]
    fn only_types_that_cannot_run_script_open_in_place() {
        for ct in [
            "image/png",
            "image/JPEG",
            "application/pdf",
            "text/plain",
            "audio/mpeg",
            "video/mp4",
        ] {
            assert!(renders_inert(ct), "{ct} should open in place");
        }
        for ct in [
            "text/html",
            "image/svg+xml",
            "application/xhtml+xml",
            "text/xml",
            "application/xml",
            "text/javascript",
            "application/octet-stream",
            "multipart/mixed",
        ] {
            assert!(!renders_inert(ct), "{ct} must download");
        }
        assert!(content_disposition("x.html", false).starts_with("attachment;"));
        assert!(content_disposition("x.png", true).starts_with("inline;"));
    }
}
