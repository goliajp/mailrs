//! `POST /api/send/multipart` — outbound send with attachments
//! and inline image resolution.

use std::sync::Arc;

use crate::message_util;

use axum::Json;
use axum::extract::{Multipart, State};
use axum::response::IntoResponse;
use rand_core::RngCore;

use super::super::common::{
    AttachmentData, build_rfc5322_with_attachments, deliver_message, resolve_thread_reply,
    verify_sender,
};
use super::super::{AuthUser, SendResult, WebState};
use super::resolve_inline_images;

pub(crate) async fn send_message_multipart(
    AuthUser {
        address: user,
        permissions,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut from = String::new();
    let mut to: Vec<String> = Vec::new();
    let mut cc: Vec<String> = Vec::new();
    let mut bcc: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut body = String::new();
    let mut html_body: Option<String> = None;
    let mut in_reply_to: Option<String> = None;
    let mut reply_to_thread_id: Option<String> = None;
    let mut attachments: Vec<AttachmentData> = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "from" => from = field.text().await.unwrap_or_default(),
            "to" => to.push(field.text().await.unwrap_or_default()),
            "cc" => cc.push(field.text().await.unwrap_or_default()),
            "bcc" => bcc.push(field.text().await.unwrap_or_default()),
            "subject" => subject = field.text().await.unwrap_or_default(),
            "body" => body = field.text().await.unwrap_or_default(),
            "html_body" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    html_body = Some(val);
                }
            }
            "in_reply_to" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    in_reply_to = Some(val);
                }
            }
            "reply_to_thread_id" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    reply_to_thread_id = Some(val);
                }
            }
            "attachments" => {
                let filename = field.file_name().unwrap_or("unnamed").to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                if let Ok(data) = field.bytes().await {
                    attachments.push(AttachmentData {
                        filename,
                        content_type,
                        data: data.to_vec(),
                    });
                }
            }
            _ => {}
        }
    }

    if from.is_empty() {
        from = user.clone();
    }

    // verify sender matches authenticated user
    if let Err(msg) = verify_sender(&from, &user, &permissions) {
        return Json(SendResult {
            success: false,
            message: Some(msg.into()),
            message_id: None,
        });
    }

    if to.is_empty() {
        return Json(SendResult {
            success: false,
            message: Some("to is required".into()),
            message_id: None,
        });
    }

    let total_recipients = to.len() + cc.len() + bcc.len();
    if total_recipients > crate::web::MAX_RECIPIENTS {
        return Json(SendResult {
            success: false,
            message: Some(format!(
                "too many recipients (max {})",
                crate::web::MAX_RECIPIENTS
            )),
            message_id: None,
        });
    }

    if body.len() > crate::web::MAX_EMAIL_BODY_LEN {
        return Json(SendResult {
            success: false,
            message: Some("message body too large".into()),
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
        reply_to_thread_id.as_deref(),
        in_reply_to.as_deref(),
        &from,
        state.mailbox_store.as_deref(),
    )
    .await;

    // append quoted text from original message for replies
    let in_reply_to_ref = resolved_in_reply_to.as_deref();
    let body_with_quote = match (in_reply_to_ref, state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            if let Some(orig) = mb_store
                .find_message_by_message_id(&from, reply_to)
                .await
                .ok()
                .flatten()
            {
                if let Some(raw_orig) =
                    message_util::read_message_raw(&state.maildir_root, &from, &orig.maildir_id)
                {
                    let (text_body, _, _) = message_util::parse_message(&raw_orig);
                    if let Some(text) = text_body {
                        let sender = message_util::decode_header(&orig.sender);
                        let date = chrono::DateTime::from_timestamp(orig.internal_date, 0)
                            .map(|dt| dt.format("%a, %d %b %Y %H:%M").to_string())
                            .unwrap_or_default();
                        let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
                        format!("{body}\n\nOn {date}, {sender} wrote:\n{quoted}")
                    } else {
                        body
                    }
                } else {
                    body
                }
            } else {
                body
            }
        }
        _ => body,
    };

    // resolve inline images from HTML before building MIME
    let (resolved_html, inline_images) = match html_body.as_deref() {
        Some(html) => {
            let (rewritten, images) =
                resolve_inline_images(html, &state.maildir_root, &from, &state.hostname).await;
            (Some(rewritten), images)
        }
        None => (None, vec![]),
    };

    let raw = build_rfc5322_with_attachments(
        &from,
        &to,
        &cc,
        &subject,
        &body_with_quote,
        resolved_html.as_deref(),
        &message_id,
        in_reply_to_ref,
        &references,
        &now,
        &attachments,
        None,
        &inline_images,
        false,
    );

    deliver_message(
        &state,
        &from,
        &to,
        &cc,
        &bcc,
        &raw,
        &message_id,
        now.timestamp(),
    )
    .await
}
