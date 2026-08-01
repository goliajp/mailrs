//! Sending: the handlers, the queue enqueue, and the sender-side mirror.
//!
//! `mirror_send_to_sender_view` writes the copy the Send view reads and is
//! best-effort throughout, while `enqueue_outbound_at` is the fallible one
//! the send actually depends on. That asymmetry is the shape of
//! `.claude/rfcs/20260730-send-status.md`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::compose::*;
use crate::handlers::compose_attach::*;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::send_queue::*;

/// POST /api/mail/send — JSON compose form, no attachments.
pub(crate) async fn send_message(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    let from = if req.from.is_empty() {
        user.clone()
    } else {
        req.from
    };
    ensure_from_allowed(&state, &user, &from).await?;
    let mut parts = ComposeParts {
        from: from.clone(),
        to: req.to,
        cc: req.cc,
        bcc: req.bcc,
        subject: req.subject,
        body: req.body,
        html_body: req.html_body,
        in_reply_to: req.in_reply_to,
        scheduled_at: req.scheduled_at,
        forward_message_id: req.forward_message_id,
        forward_attachments_from: req.forward_attachments_from,
        reply_to_thread_id: req.reply_to_thread_id,
        attachments: Vec::new(),
    };
    // If the compose is a forward, look up the original message and
    // prepend its body onto what the user typed. Pre-2.9.38 the field
    // was passed through untouched and build_rfc5322 ignored it, so
    // every forward sent only the user's leading text — the recipient
    // saw "FYI." with none of the forwarded content.
    inline_forward_content(&state, &user, &mut parts).await;
    // A redraft carries the failed send's attachments. Unlike the forward
    // step above it can fail the request: losing them silently is the
    // whole point of not going through the drafts table.
    inline_redraft_attachments(
        &user,
        req.redraft_of.as_deref(),
        req.redraft_keep.as_deref(),
        &mut parts,
    )
    .await?;
    // Before `build_rfc5322`, which is what writes In-Reply-To/References.
    infer_in_reply_to(&state, &user, &mut parts).await;
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    recipients.extend(parts.bcc.clone());
    let (message_id, envelope) = build_rfc5322(&parts, &from);
    // The Send row lands here, inside the fallible step, so a 200
    // implies it exists. `thread_id` mirrors what
    // `mirror_send_to_sender_view` derives for a fresh send: its own
    // Message-ID when the mail starts a conversation.
    //
    // A redraft gets a **new** Message-ID, unlike a resend: the user
    // edited the content, and a Message-ID names a particular message.
    // `resent_from` keeps the chain back to what it repairs.
    let send_meta = SendMeta {
        message_id: &message_id,
        thread_id: parts.in_reply_to.as_deref().unwrap_or(&message_id),
        subject: &parts.subject,
        to: &parts.to,
        cc: &parts.cc,
        envelope_ref: "",
        resent_from: req.redraft_of.as_deref(),
    };
    enqueue_outbound_at(
        &user,
        &recipients,
        &envelope,
        parts.scheduled_at,
        Some(send_meta),
    )?;
    mirror_send_to_sender_view(&state, &user, &parts, &envelope, &message_id, false).await;
    Ok(Json(SendResponse {
        message_id,
        success: true,
        message: None,
    }))
}

/// MCP-side send helper — same pipeline as [`send_message`] but without
/// the axum/JSON wrapper so the MCP tool can drive it directly. Returns
/// the assigned Message-ID on success.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn send_email_mcp(
    state: &Arc<WebState>,
    auth_user: &str,
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    scheduled_at: Option<i64>,
) -> Result<String, String> {
    ensure_from_allowed(state, auth_user, from)
        .await
        .map_err(|c| format!("from not allowed ({c})"))?;
    let parts = ComposeParts {
        from: from.to_string(),
        to: to.to_vec(),
        cc: cc.to_vec(),
        bcc: Vec::new(),
        subject: subject.to_string(),
        body: body.to_string(),
        html_body: String::new(),
        in_reply_to: in_reply_to.map(|s| s.to_string()),
        forward_message_id: None,
        forward_attachments_from: None,
        reply_to_thread_id: None,
        attachments: Vec::new(),
        scheduled_at,
    };
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    let (message_id, envelope) = build_rfc5322(&parts, from);
    // Same call the REST send handler uses — a future `scheduled_at`
    // lands the id in the scheduled zset instead of pending.
    let send_meta = SendMeta {
        message_id: &message_id,
        thread_id: parts.in_reply_to.as_deref().unwrap_or(&message_id),
        subject: &parts.subject,
        to: &parts.to,
        cc: &parts.cc,
        envelope_ref: "",
        resent_from: None,
    };
    enqueue_outbound_at(
        auth_user,
        &recipients,
        &envelope,
        parts.scheduled_at,
        Some(send_meta),
    )
    .map_err(|c| format!("enqueue failed ({c})"))?;
    mirror_send_to_sender_view(state, auth_user, &parts, &envelope, &message_id, false).await;
    Ok(message_id)
}

/// POST /api/mail/send-multipart — multipart/form-data compose form.
/// Fields: from, to (repeated), cc (repeated), bcc (repeated), subject,
/// body, html_body, attachments (repeated file parts), in_reply_to,
/// scheduled_at, forward_message_id, forward_attachments_from.
pub(crate) async fn send_message_multipart(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<SendResponse>, StatusCode> {
    let mut parts = ComposeParts {
        from: user.clone(),
        ..Default::default()
    };
    let mut redraft_of: Option<String> = None;
    // Absent and empty are different: absent keeps every carried
    // attachment, an empty list keeps none. A form that dropped the
    // distinction would silently re-attach files the user removed.
    let mut redraft_keep: Option<Vec<usize>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "from" => parts.from = field.text().await.unwrap_or_default(),
            "to" => parts.to.push(field.text().await.unwrap_or_default()),
            "cc" => parts.cc.push(field.text().await.unwrap_or_default()),
            "bcc" => parts.bcc.push(field.text().await.unwrap_or_default()),
            "subject" => parts.subject = field.text().await.unwrap_or_default(),
            "body" => parts.body = field.text().await.unwrap_or_default(),
            "html_body" => parts.html_body = field.text().await.unwrap_or_default(),
            "in_reply_to" => parts.in_reply_to = Some(field.text().await.unwrap_or_default()),
            "forward_message_id" => {
                parts.forward_message_id = Some(field.text().await.unwrap_or_default())
            }
            "forward_attachments_from" => {
                parts.forward_attachments_from =
                    field.text().await.ok().and_then(|s| s.trim().parse().ok())
            }
            "reply_to_thread_id" => {
                parts.reply_to_thread_id = field.text().await.ok().filter(|s| !s.is_empty())
            }
            "scheduled_at" => {
                let raw = field.text().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                parts.scheduled_at =
                    parse_scheduled_at(&raw).map_err(|_| StatusCode::BAD_REQUEST)?;
            }
            "redraft_of" => redraft_of = field.text().await.ok().filter(|s| !s.is_empty()),
            // One comma-separated field, not a repeated one. Repeating it
            // cannot express "keep none": zero occurrences and an empty
            // selection would both arrive as no field at all, and the two
            // mean opposite things. Present-but-empty says none; absent
            // says all.
            "redraft_keep" => {
                redraft_keep = Some(
                    field
                        .text()
                        .await
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(|s| s.trim().parse::<usize>().ok())
                        .collect(),
                )
            }
            "attachments" => {
                let filename = field.file_name().unwrap_or("attachment").to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
                parts.attachments.push(Attachment {
                    filename,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            }
            _ => {
                let _ = field.text().await;
            }
        }
    }
    if parts.from.is_empty() {
        parts.from = user.clone();
    }
    ensure_from_allowed(&state, &user, &parts.from).await?;
    // Same fix as the JSON handler: prepend the forwarded original
    // when the request carries forward_message_id.
    inline_forward_content(&state, &user, &mut parts).await;
    inline_redraft_attachments(
        &user,
        redraft_of.as_deref(),
        redraft_keep.as_deref(),
        &mut parts,
    )
    .await?;
    infer_in_reply_to(&state, &user, &mut parts).await;
    let mut recipients = parts.to.clone();
    recipients.extend(parts.cc.clone());
    recipients.extend(parts.bcc.clone());
    let from = parts.from.clone();
    let (message_id, envelope) = build_rfc5322(&parts, &from);
    // The Send row lands here, inside the fallible step, so a 200
    // implies it exists. `thread_id` mirrors what
    // `mirror_send_to_sender_view` derives for a fresh send: its own
    // Message-ID when the mail starts a conversation.
    let send_meta = SendMeta {
        message_id: &message_id,
        thread_id: parts.in_reply_to.as_deref().unwrap_or(&message_id),
        subject: &parts.subject,
        to: &parts.to,
        cc: &parts.cc,
        envelope_ref: "",
        resent_from: redraft_of.as_deref(),
    };
    enqueue_outbound_at(
        &user,
        &recipients,
        &envelope,
        parts.scheduled_at,
        Some(send_meta),
    )?;
    mirror_send_to_sender_view(&state, &user, &parts, &envelope, &message_id, false).await;
    Ok(Json(SendResponse {
        message_id,
        success: true,
        message: None,
    }))
}
