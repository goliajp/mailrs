//! Outbound send pipeline: JSON send, multipart send (with attachments),
//! deliverability checks, and cancel-pending-send.

use std::sync::Arc;

use axum::extract::{Multipart, State};
use axum::response::IntoResponse;
use axum::Json;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::message_util;

use super::common::{
    build_rfc5322_with_attachments, deliver_message, deliver_message_ex, resolve_thread_reply,
    verify_sender, AttachmentData,
};
use super::{ApiResult, AuthUser, SendResult, WebState};

#[derive(Deserialize)]
pub(crate) struct SendMessageRequest {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
    #[serde(default)]
    pub list_unsubscribe: Option<String>,
    /// optional ISO 8601 timestamp for scheduled delivery
    #[serde(default)]
    pub scheduled_at: Option<String>,
    /// request a read receipt (MDN) from recipients
    #[serde(default)]
    pub request_read_receipt: bool,
    /// uid of original message to forward attachments from (legacy, prefer forward_message_id)
    #[serde(default)]
    pub forward_attachments_from: Option<u32>,
    /// message-id header of original message to forward (more reliable than uid)
    #[serde(default)]
    pub forward_message_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct DeliverabilityCheckRequest {
    pub recipient: String,
}

#[derive(Serialize)]
pub(crate) struct DeliverabilityCheckResult {
    pub recipient: String,
    pub suppressed: bool,
    pub mx_found: bool,
    pub mx_hosts: Vec<String>,
    pub issues: Vec<String>,
}

pub(crate) async fn send_message(
    AuthUser { address: user, permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    // debug: log forward params
    eprintln!(
        "send_message: user={user} forward_message_id={:?} forward_attachments_from={:?} subject={:?} body_len={}",
        req.forward_message_id,
        req.forward_attachments_from,
        req.subject,
        req.body.len(),
    );

    if req.to.is_empty() {
        return Json(SendResult {
            success: false,
            message: Some("to is required".into()),
            message_id: None,
        });
    }

    let total_recipients = req.to.len() + req.cc.len() + req.bcc.len();
    if total_recipients > super::MAX_RECIPIENTS {
        return Json(SendResult {
            success: false,
            message: Some(format!("too many recipients (max {})", super::MAX_RECIPIENTS)),
            message_id: None,
        });
    }

    if req.body.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(SendResult {
            success: false,
            message: Some("message body too large".into()),
            message_id: None,
        });
    }

    if req.subject.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(SendResult {
            success: false,
            message: Some("subject too long".into()),
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
    let body_with_quote = match (in_reply_to_ref, state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => {
            if let Some(orig) = mb_store
                .find_message_by_message_id(from, reply_to)
                .await
                .ok()
                .flatten()
            {
                if let Some(raw_orig) =
                    message_util::read_message_raw(&state.maildir_root, from, &orig.maildir_id)
                {
                    let (text_body, _, _) = message_util::parse_message(&raw_orig);
                    if let Some(text) = text_body {
                        let sender = message_util::decode_header(&orig.sender);
                        let date = chrono::DateTime::from_timestamp(orig.internal_date, 0)
                            .map(|dt| dt.format("%a, %d %b %Y %H:%M").to_string())
                            .unwrap_or_default();
                        let quoted: String = text.lines().map(|l| format!("> {l}\n")).collect();
                        format!("{}\n\nOn {date}, {sender} wrote:\n{quoted}", req.body)
                    } else {
                        req.body.clone()
                    }
                } else {
                    req.body.clone()
                }
            } else {
                req.body.clone()
            }
        }
        _ => req.body.clone(),
    };

    // when forwarding, build email from original raw message (full body + all attachments)
    // prefer forward_message_id (globally unique), fall back to uid
    let forward_requested = req.forward_message_id.is_some() || req.forward_attachments_from.is_some();
    let (final_body, final_html, forwarded_attachments) = if forward_requested {
        let (orig_text, orig_html, atts) = extract_full_forward_by_id(
            &state,
            &user,  // always use authenticated user, not from
            req.forward_message_id.as_deref(),
            req.forward_attachments_from,
        ).await;
        // prepend user's message before the forwarded content
        let user_text = req.body.clone();
        let fwd_text = if let Some(ref text) = orig_text {
            format!("{user_text}\n\n---------- Forwarded message ----------\n{text}")
        } else {
            user_text.clone()
        };
        let user_html_fallback = format!("<p>{}</p>", user_text.replace('\n', "<br>"));
        let user_html_str = req.html_body.as_deref().unwrap_or(&user_html_fallback);
        let fwd_html = if let Some(ref html) = orig_html {
            Some(format!("{user_html_str}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><div style=\"color:#555\">{html}</div>"))
        } else if let Some(ref text) = orig_text {
            let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br>");
            Some(format!("{user_html_str}<hr style=\"border:none;border-top:1px solid #ccc;margin:16px 0\"><pre style=\"font-family:sans-serif;white-space:pre-wrap\">{escaped}</pre>"))
        } else {
            req.html_body.clone()
        };
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
    let Some(ref mb_store) = state.mailbox_store else { return empty; };

    // find message: try message_id first, then uid
    let meta = if let Some(mid) = message_id {
        match mb_store.find_message_by_message_id(user, mid).await {
            Ok(Some(m)) => {
                eprintln!("forward: found by message_id={mid} user={user}");
                m
            }
            _ => {
                eprintln!("forward: message_id={mid} not found for user={user}, trying uid");
                if let Some(u) = uid {
                    match mb_store.find_message_by_uid(user, u).await {
                        Ok(Some(m)) => m,
                        _ => {
                            eprintln!("forward: uid={u} also not found for user={user}");
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
                eprintln!("forward: found by uid={u} user={user}");
                m
            }
            _ => {
                eprintln!("forward: uid={u} not found for user={user}");
                return empty;
            }
        }
    } else {
        eprintln!("forward: no message_id or uid provided");
        return empty;
    };

    let Some(raw) = message_util::read_message_raw(&state.maildir_root, user, &meta.maildir_id) else {
        eprintln!("forward: raw message not found maildir_id={} user={user}", meta.maildir_id);
        return empty;
    };
    eprintln!("forward: raw message loaded, {} bytes", raw.len());

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

        let filename = part.attachment_filename().unwrap_or("attachment").to_string();
        let content_type = if mt.is_empty() || mt == "/" {
            "application/octet-stream".to_string()
        } else {
            mt
        };

        if !part.body.is_empty() {
            attachments.push(AttachmentData {
                filename,
                content_type,
                data: part.body.clone(),
            });
        }
    }

    eprintln!(
        "forward: uid={uid:?} text={}bytes html={}bytes attachments={}",
        text_body.as_ref().map(|s| s.len()).unwrap_or(0),
        html_body.as_ref().map(|s| s.len()).unwrap_or(0),
        attachments.len()
    );

    (text_body, html_body, attachments)
}

/// resolve inline images from HTML, load from disk, return images + rewritten HTML
async fn resolve_inline_images(
    html: &str,
    maildir_root: &str,
    user_address: &str,
    hostname: &str,
) -> (String, Vec<crate::inline_image::InlineImage>) {
    let ids = crate::inline_image::find_inline_urls(html);
    if ids.is_empty() {
        return (html.to_string(), vec![]);
    }

    let mut images = Vec::new();
    for id in &ids {
        // try all known extensions
        for ext in &["png", "jpg", "webp", "gif", "tiff", "bmp", "svg", "bin"] {
            let path =
                crate::inline_image::inline_path(maildir_root, user_address, id, ext);
            if let Ok(data) = tokio::fs::read(&path).await {
                let content_type = match *ext {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "webp" => "image/webp",
                    "gif" => "image/gif",
                    "tiff" => "image/tiff",
                    "bmp" => "image/bmp",
                    "svg" => "image/svg+xml",
                    _ => "application/octet-stream",
                };
                images.push(crate::inline_image::InlineImage {
                    id: id.clone(),
                    content_type: content_type.to_string(),
                    data,
                    cid: format!("{id}@{hostname}"),
                });
                break;
            }
        }
    }

    let rewritten = crate::inline_image::replace_inline_urls_with_cid(html, &images);
    (rewritten, images)
}

pub(crate) async fn send_message_multipart(
    AuthUser { address: user, permissions, .. }: AuthUser,
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
    if total_recipients > super::MAX_RECIPIENTS {
        return Json(SendResult {
            success: false,
            message: Some(format!("too many recipients (max {})", super::MAX_RECIPIENTS)),
            message_id: None,
        });
    }

    if body.len() > super::MAX_EMAIL_BODY_LEN {
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

/// cancel a pending outbound message (undo send)
pub(crate) async fn cancel_pending_send(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    axum::extract::Path(message_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if message_id.is_empty() || message_id.len() > super::MAX_PATH_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("invalid message_id".into()),
        });
    }

    let Some(ref pool) = state.outbound_queue else {
        return Json(ApiResult {
            success: false,
            message: Some("outbound queue not configured".into()),
        });
    };

    match mailrs_outbound_queue::queue::cancel_pending_by_message_id(pool, &message_id, &user).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("message not found or already sent".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(format!("failed to cancel: {e}")),
        }),
    }
}

pub(crate) async fn check_deliverability(
    _auth: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<DeliverabilityCheckRequest>,
) -> impl IntoResponse {
    let mut issues = Vec::new();
    let recipient = req.recipient.trim().to_lowercase();

    // check suppression list
    let suppressed = if let Some(ref pool) = state.outbound_queue {
        mailrs_outbound_queue::queue::is_suppressed(pool, &recipient).await
    } else {
        false
    };
    if suppressed {
        issues.push("recipient is on suppression list (previous hard bounce)".into());
    }

    // check MX records
    let domain = recipient.split_once('@').map(|(_, d)| d).unwrap_or("");
    let (mx_found, mx_hosts) = if let Some(ref resolver) = state.resolver {
        match mailrs_smtp_client::resolve_mx(resolver, domain).await {
            Ok(records) => (true, records.iter().map(|r| r.exchange.clone()).collect()),
            Err(e) => {
                issues.push(format!("MX lookup failed: {e}"));
                (false, vec![])
            }
        }
    } else {
        issues.push("DNS resolver not available".into());
        (false, vec![])
    };

    if domain.is_empty() {
        issues.push("invalid email address".into());
    }

    Json(DeliverabilityCheckResult {
        recipient,
        suppressed,
        mx_found,
        mx_hosts,
        issues,
    })
}
