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
    let mut wire = resolve_message(&state, &user, uid).await?;
    wire.flags |= 0b0000_1000; // FLAG_DELETED
    let msg_id = wire.message_id.clone();
    let json = serde_json::to_vec(&wire).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("mailrs:msg:{msg_id}");
    let json_c = json.clone();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(b"blob" as &[u8], json_c.as_slice())])?;
        Ok(())
    });
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
        let ids = c.lrange(b"mailrs:outbound:pending", 0, -1)?;
        let mut removed = 0u32;
        let mut keep = Vec::new();
        for id_bytes in ids {
            let Ok(id_str) = std::str::from_utf8(&id_bytes) else {
                continue;
            };
            let hkey = format!("mailrs:outbound:{id_str}");
            let blob = c.hget(hkey.as_bytes(), b"blob")?;
            let mut matched = false;
            if let Some(bytes) = blob {
                let s = String::from_utf8_lossy(&bytes);
                if s.contains(&format!("<{target}>")) || s.contains(&target)
                {
                    // Only cancel entries owned by the requesting user.
                    if s.contains(&format!("\"sender\":\"{user_c}\"")) {
                        removed += 1;
                        matched = true;
                        c.del(&[hkey.as_bytes()])?;
                    }
                }
            }
            if !matched {
                keep.push(id_bytes);
            }
        }
        c.del(&[b"mailrs:outbound:pending".as_slice()])?;
        for id in keep {
            c.lpush(b"mailrs:outbound:pending", &[id.as_slice()])?;
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
    let mut wire = resolve_message(&state, &user, uid).await?;
    let new_bits = flag_string_to_bits(&req.flags);
    let old_bits = wire.flags;
    wire.flags = new_bits;

    // Re-serialize the wire back into the mailrs:msg:<mid> hash.
    let msg_id = wire.message_id.clone();
    let json = serde_json::to_vec(&wire).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let key = format!("mailrs:msg:{msg_id}");
    // Fastcore's embedded kevy — reach through core-api? Actually the
    // message hash lives on the fastcore-side embedded kevy since
    // upsert_message writes there. But we can't call it from webapi
    // without a dedicated RPC. Fall through: try the network kevy
    // path as a best-effort mirror; if the read/write pair is on
    // embedded kevy the value will diverge until fastcore is invoked.
    // Correct long-term fix is `POST /v1/users/{user}/messages/{uid}/flags`
    // fastcore RPC. Deferred to a follow-up patch.
    let key_c = key.clone();
    let json_c = json.clone();
    let _ = crate::handlers::kevy_util::with_kevy(move |c| {
        c.hset(key_c.as_bytes(), &[(b"blob" as &[u8], json_c.as_slice())])?;
        Ok(())
    });

    // \Seen toggle reconciliation on the thread aggregate. Call
    // fastcore mark_read / mark_unread which own the has_unread zset.
    let seen_bit = 0b0000_0001;
    let was_seen = (old_bits & seen_bit) != 0;
    let is_seen = (new_bits & seen_bit) != 0;
    if was_seen != is_seen && !wire.thread_id.is_empty() {
        if is_seen {
            let _ = state.fast().mark_thread_read(&user, &wire.thread_id).await;
        } else {
            let _ = state
                .fast()
                .mark_thread_unread(&user, &wire.thread_id)
                .await;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
