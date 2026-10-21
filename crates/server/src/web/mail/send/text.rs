//! JSON `POST /api/send` — text-only outbound send (no
//! attachments). For multipart with attachments see `multipart.rs`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use rand_core::RngCore;

use crate::message_util;

use super::super::common::{
    AttachmentData, build_rfc5322_with_attachments, deliver_message_ex, resolve_thread_reply,
    verify_sender,
};
use super::super::{AuthUser, SendResult, WebState};
use super::{SendMessageRequest, resolve_inline_images};

mod compose;

pub(crate) async fn send_message(
    AuthUser {
        address: user,
        permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    tracing::debug!(
        event = "send_message",
        user = %user,
        forward_message_id = ?req.forward_message_id,
        forward_attachments_from = ?req.forward_attachments_from,
        subject = ?req.subject,
        body_len = req.body.len()
    );

    if let Some(message) = compose::request_error(&req) {
        return Json(SendResult {
            success: false,
            message: Some(message),
            message_id: None,
        });
    }

    // use authenticated user as sender
    let from = if req.from.is_empty() {
        &user
    } else {
        &req.from
    };

    // verify sender matches authenticated user
    if let Err(msg) = verify_sender(from, &user, &permissions) {
        return Json(SendResult {
            success: false,
            message: Some(msg.into()),
            message_id: None,
        });
    }

    let now = chrono::Utc::now();
    let message_id = format!(
        "{}.{}@{}",
        now.timestamp_millis(),
        rand_core::OsRng.next_u32(),
        state.hostname
    );

    // resolve reply: thread_id -> in_reply_to, or use explicit in_reply_to
    let (resolved_in_reply_to, references) = resolve_thread_reply(
        req.reply_to_thread_id.as_deref(),
        req.in_reply_to.as_deref(),
        from,
        state.mailbox_store.as_deref(),
    )
    .await;

    // append quoted text from original message for replies
    let in_reply_to_ref = resolved_in_reply_to.as_deref();
    let body_with_quote = compose::body_with_quote(&state, from, in_reply_to_ref, &req.body).await;

    // when forwarding, build email from original raw message (full body + all attachments)
    // prefer forward_message_id (globally unique), fall back to uid
    let forward_requested =
        req.forward_message_id.is_some() || req.forward_attachments_from.is_some();
    let (final_body, final_html, forwarded_attachments) = if forward_requested {
        let (orig_text, orig_html, atts) = extract_full_forward_by_id(
            &state,
            &user, // always use authenticated user, not from
            req.forward_message_id.as_deref(),
            req.forward_attachments_from,
        )
        .await;
        let (fwd_text, fwd_html) = compose::forward_bodies(&req, &orig_text, &orig_html);
        (fwd_text, fwd_html, atts)
    } else {
        (body_with_quote.clone(), req.html_body.clone(), vec![])
    };

    // resolve inline images from HTML before building MIME
    let (resolved_html, inline_images) = match final_html.as_deref() {
        Some(html) => {
            let (rewritten, images) =
                resolve_inline_images(html, &state.maildir_root, from, &state.hostname).await;
            (Some(rewritten), images)
        }
        None => (None, vec![]),
    };

    let raw = build_rfc5322_with_attachments(
        from,
        &req.to,
        &req.cc,
        &req.subject,
        &final_body,
        resolved_html.as_deref(),
        &message_id,
        in_reply_to_ref,
        &references,
        &now,
        &forwarded_attachments,
        req.list_unsubscribe.as_deref(),
        &inline_images,
        req.request_read_receipt,
    );

    // parse optional scheduled_at for send-later
    let scheduled_at = req.scheduled_at.as_deref().and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|dt| dt.timestamp())
    });

    deliver_message_ex(
        &state,
        from,
        &req.to,
        &req.cc,
        &req.bcc,
        &raw,
        &message_id,
        now.timestamp(),
        scheduled_at,
    )
    .await
}

/// extract full body (text + html) and all attachments from an existing message for forwarding
/// tries message_id first (globally unique), falls back to uid
async fn extract_full_forward_by_id(
    state: &WebState,
    user: &str,
    message_id: Option<&str>,
    uid: Option<u32>,
) -> (Option<String>, Option<String>, Vec<AttachmentData>) {
    let empty = (None, None, vec![]);
    let Some(ref mb_store) = state.mailbox_store else {
        return empty;
    };

    // find message: try message_id first, then uid
    let meta = if let Some(mid) = message_id {
        match mb_store.find_message_by_message_id(user, mid).await {
            Ok(Some(m)) => {
                tracing::debug!(event = "forward_found_by_msgid", message_id = %mid, user = %user);
                m
            }
            _ => {
                tracing::debug!(event = "forward_msgid_miss", message_id = %mid, user = %user);
                if let Some(u) = uid {
                    match mb_store.find_message_by_uid(user, u).await {
                        Ok(Some(m)) => m,
                        _ => {
                            tracing::warn!(event = "forward_uid_miss", uid = u, user = %user);
                            return empty;
                        }
                    }
                } else {
                    return empty;
                }
            }
        }
    } else if let Some(u) = uid {
        match mb_store.find_message_by_uid(user, u).await {
            Ok(Some(m)) => {
                tracing::debug!(event = "forward_found_by_uid", uid = u, user = %user);
                m
            }
            _ => {
                tracing::warn!(event = "forward_uid_miss", uid = u, user = %user);
                return empty;
            }
        }
    } else {
        tracing::warn!(event = "forward_no_identifier", user = %user);
        return empty;
    };

    let Some(raw) =
        message_util::read_message_raw(&state.maildir_root, user, &meta.maildir_id).await
    else {
        tracing::warn!(
            event = "forward_raw_not_found",
            maildir_id = %meta.maildir_id,
            user = %user
        );
        return empty;
    };
    tracing::debug!(event = "forward_raw_loaded", bytes = raw.len());

    // use the existing parser that handles nested MIME well
    let (text_body, html_body, _) = message_util::parse_message(&raw);

    // parse attachments from raw MIME via mailrs-mime
    let mut attachments = Vec::new();
    let parsed = mailrs_mime::parse(&raw);
    for part in parsed.walk() {
        let mt = part.content_type.mime_type();
        // Skip the root node + text body parts (only iterate leaf
        // attachments). text/plain + text/html WITHOUT attachment
        // disposition are body parts.
        if part.content_type.is_multipart() {
            continue;
        }
        let is_text_body_part = (mt == "text/plain" || mt == "text/html")
            && part
                .disposition
                .as_ref()
                .map(|d| !d.is_attachment())
                .unwrap_or(true);
        if is_text_body_part {
            continue;
        }

        let filename = part
            .attachment_filename()
            .unwrap_or("attachment")
            .to_string();
        let content_type = if mt.is_empty() || mt == "/" {
            "application/octet-stream".to_string()
        } else {
            mt
        };

        if !part.body.is_empty() {
            attachments.push(AttachmentData {
                filename,
                content_type,
                data: part.body.to_vec(),
            });
        }
    }

    tracing::debug!(
        event = "forward_extracted",
        uid = ?uid,
        text_bytes = text_body.as_ref().map(|s| s.len()).unwrap_or(0),
        html_bytes = html_body.as_ref().map(|s| s.len()).unwrap_or(0),
        attachment_count = attachments.len()
    );

    (text_body, html_body, attachments)
}
