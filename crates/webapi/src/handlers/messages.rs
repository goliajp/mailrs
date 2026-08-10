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
use mailrs_core_sidestate::families::outbound::{PENDING_IDX, SCHEDULED_IDX};

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
        // RFC 6266. A header value must be ASCII, and this filename has
        // just been decoded — the message that prompted the decode was
        // Japanese, so interpolating it raw would make `body()` return
        // Err and the download answer 500. The ASCII `filename=` stays
        // for readers that ignore the extended form.
        .header("content-disposition", content_disposition(&filename))
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

/// DELETE /api/mail/pending/{message_id} — cancel a queued outbound.
/// Walks the pending list looking for an id whose blob's Message-ID
/// header matches, then removes the id + blob. Idempotent.
pub async fn cancel_pending_send(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(message_id): Path<String>,
) -> Json<serde_json::Value> {
    let target = message_id;
    let user_c = user.clone();
    let removed = crate::handlers::kevy_util::with_kevy(move |c| {
        // v2: pending is `mailrs:outbound:pending-idx` and the job row
        // is `mailrs:outbound:job:{id}` — the pre-2.9.38 form read the
        // legacy `pending` list + `mailrs:outbound:{id}` blob, which
        // sender no longer produces, so cancel-pending walked an empty
        // list and never matched anything real.
        let ids = c
            .lrange(PENDING_IDX, 0, -1)
            .map_err(std::io::Error::other)?;
        let mut removed = 0u32;
        let mut keep = Vec::new();
        for id_bytes in ids {
            let Ok(id_str) = std::str::from_utf8(&id_bytes) else {
                keep.push(id_bytes);
                continue;
            };
            let hkey = format!("mailrs:outbound:job:{id_str}");
            let blob = c
                .hget(hkey.as_bytes(), b"blob")
                .map_err(std::io::Error::other)?;
            // Strict JSON compare: parse the envelope, match on the
            // sender field AND the Message-ID header extracted from
            // message_data. Prior contains-in-string version would
            // false-positive on any Message-ID substring, letting the
            // caller cancel other users' outbound entries.
            let mut matched = false;
            if let Some(bytes) = blob
                && let Ok(env) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                let sender = env.get("sender").and_then(|v| v.as_str()).unwrap_or("");
                // v2 blobs always carry message_data_b64.
                let md_owned: String =
                    if let Some(b64) = env.get("message_data_b64").and_then(|v| v.as_str()) {
                        use base64::Engine as _;
                        match base64::engine::general_purpose::STANDARD.decode(b64) {
                            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                            Err(_) => String::new(),
                        }
                    } else {
                        env.get("message_data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    };
                let header = format!("Message-ID: <{target}>\r\n");
                if sender == user_c && md_owned.contains(&header) {
                    removed += 1;
                    matched = true;
                    // Drop the job hash so a re-drain of pending-idx
                    // sees try_claim's state check fail.
                    c.del(&[hkey.as_bytes()]).map_err(std::io::Error::other)?;
                }
            }
            if !matched {
                keep.push(id_bytes);
            }
        }
        c.del(&[PENDING_IDX]).map_err(std::io::Error::other)?;
        for id in keep {
            c.lpush(PENDING_IDX, &[id.as_slice()])
                .map_err(std::io::Error::other)?;
        }
        // also sweep the scheduled zset — a scheduled send hasn't reached
        // the pending list yet, so cancelling it must look here too (G13).
        // v2 scheduled key is `scheduled-idx` (see stone outbound.rs).
        let sched = c
            .zrange(SCHEDULED_IDX, 0, -1)
            .map_err(std::io::Error::other)?;
        for id_bytes in sched {
            let Ok(id_str) = std::str::from_utf8(&id_bytes) else {
                continue;
            };
            let hkey = format!("mailrs:outbound:job:{id_str}");
            let Some(bytes) = c
                .hget(hkey.as_bytes(), b"blob")
                .map_err(std::io::Error::other)?
            else {
                continue;
            };
            let Ok(env) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let sender = env.get("sender").and_then(|v| v.as_str()).unwrap_or("");
            let md: String = env
                .get("message_data_b64")
                .and_then(|v| v.as_str())
                .and_then(|b64| {
                    use base64::Engine as _;
                    base64::engine::general_purpose::STANDARD.decode(b64).ok()
                })
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let header = format!("Message-ID: <{target}>\r\n");
            if sender == user_c && md.contains(&header) {
                c.zrem(SCHEDULED_IDX, &[id_bytes.as_slice()])
                    .map_err(std::io::Error::other)?;
                c.del(&[hkey.as_bytes()]).map_err(std::io::Error::other)?;
                removed += 1;
            }
        }
        Ok(removed)
    })
    .unwrap_or(0);
    Json(serde_json::json!({
        "success": removed > 0,
        "message": if removed == 0 { Some("message not found or already sent") } else { None },
    }))
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
fn content_disposition(filename: &str) -> String {
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
        return format!("inline; {encoded}");
    }
    format!("inline; filename=\"{ascii}\"; {encoded}")
}

#[cfg(test)]
mod tests {
    use super::blob_ref_location;
    use super::content_disposition;

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
        let h = content_disposition("源泉徴収票(2025年分。ZHANG FAN様).pdf");
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
        let h = content_disposition(r#"a"; evil="x.pdf"#);
        assert_eq!(
            h, r#"inline; filename="a\"; evil=\"x.pdf""#,
            "the quotes are escaped, so there is still exactly one parameter"
        );
    }
}
