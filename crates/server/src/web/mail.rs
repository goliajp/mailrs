use std::sync::Arc;

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use base64::Engine;
use mail_parser::MimeHeaders;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::message_util;

use super::{ApiResult, AuthUser, SendResult, WebState};

// --- draft types ---

#[derive(Deserialize)]
pub(super) struct SaveDraftRequest {
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

#[derive(Serialize)]
pub(super) struct DraftInfo {
    pub id: i64,
    pub to_addresses: String,
    pub cc_addresses: String,
    pub bcc_addresses: String,
    pub subject: String,
    pub body: String,
    pub reply_to_thread_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub(super) struct SaveDraftResult {
    pub success: bool,
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Serialize)]
pub(super) struct FolderInfo {
    pub name: String,
    pub total: u32,
    pub unseen: u32,
    pub uidnext: u32,
}

#[derive(Serialize)]
pub(super) struct MessageSummary {
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub size: u32,
    pub flags: u32,
    pub internal_date: i64,
}

#[derive(Serialize)]
pub(super) struct MessageDetail {
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub size: u32,
    pub flags: u32,
    pub internal_date: i64,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<crate::message_util::AttachmentInfo>,
    pub category: String,
    pub risk_score: u8,
    pub risk_reason: String,
    pub summary: String,
    pub people: serde_json::Value,
    pub dates: serde_json::Value,
    pub amounts: serde_json::Value,
    pub action_items: serde_json::Value,
    pub ai_analyzed: bool,
    pub clean_text: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct FolderMessagesQuery {
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

#[derive(Deserialize)]
pub(super) struct FlagUpdate {
    pub action: String,
    pub flags: u32,
}

#[derive(Deserialize)]
pub(super) struct SendMessageRequest {
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
    /// uid of original message to forward attachments from
    #[serde(default)]
    pub forward_attachments_from: Option<u32>,
}

pub(crate) struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub(super) async fn get_folders(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<FolderInfo>::new());
    };

    // auto-create default mailboxes on first access
    let _ = mb_store.ensure_default_mailboxes(&user).await;

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    let mut folders = Vec::with_capacity(mailboxes.len());
    for mb in &mailboxes {
        let (total, unseen) = mb_store.mailbox_status(mb.id).await.unwrap_or((0, 0));
        folders.push(FolderInfo {
            name: mb.name.clone(),
            total,
            unseen,
            uidnext: mb.uidnext,
        });
    }

    Json(folders)
}

pub(super) async fn get_folder_messages(
    Path(name): Path<String>,
    Query(q): Query<FolderMessagesQuery>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<MessageSummary>::new());
    };

    if name.len() > super::MAX_PATH_LEN {
        return Json(Vec::<MessageSummary>::new());
    }

    let limit = super::clamp_limit(q.limit);
    let offset = super::clamp_offset(q.offset);

    let mb = match mb_store.get_mailbox(&user, &name).await {
        Ok(Some(mb)) => mb,
        _ => return Json(Vec::<MessageSummary>::new()),
    };

    let messages = mb_store
        .list_messages(mb.id, offset, limit)
        .await
        .unwrap_or_default();

    let summaries: Vec<MessageSummary> = messages
        .iter()
        .map(|msg| MessageSummary {
            uid: msg.uid,
            sender: message_util::decode_header(&msg.sender),
            recipients: msg.recipients.clone(),
            subject: message_util::decode_header(&msg.subject),
            size: msg.size,
            flags: msg.flags,
            internal_date: msg.internal_date,
        })
        .collect();

    Json(summaries)
}

pub(super) async fn get_message(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(None::<MessageDetail>);
    };

    // find the message across all mailboxes
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
            let parsed = raw
                .as_deref()
                .map(message_util::parse_message)
                .unwrap_or_default();
            let sender = message_util::decode_header(&msg.sender);
            let subject = message_util::decode_header(&msg.subject);

            // try AI analysis first, fall back to rule-based
            let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
            let (
                category,
                risk_score,
                risk_reason,
                summary,
                people,
                dates,
                amounts,
                action_items,
                ai_analyzed,
                clean_text,
            ) = if let Some(ref a) = ai {
                let ct = if a.clean_text.is_empty() {
                    None
                } else {
                    Some(a.clean_text.clone())
                };
                (
                    a.category.clone(),
                    a.risk_score as u8,
                    a.risk_reason.clone(),
                    a.summary.clone(),
                    a.people.clone(),
                    a.dates.clone(),
                    a.amounts.clone(),
                    a.action_items.clone(),
                    true,
                    ct,
                )
            } else {
                let (cat, score) = super::classify_email(
                    &sender,
                    &subject,
                    parsed.0.as_deref(),
                    parsed.1.as_deref(),
                );
                (
                    cat,
                    score,
                    String::new(),
                    String::new(),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    serde_json::json!([]),
                    false,
                    None,
                )
            };

            return Json(Some(MessageDetail {
                uid: msg.uid,
                sender,
                recipients: msg.recipients,
                subject,
                size: msg.size,
                flags: msg.flags,
                internal_date: msg.internal_date,
                text_body: parsed.0,
                html_body: parsed.1,
                attachments: parsed.2,
                category,
                risk_score,
                risk_reason,
                summary,
                people,
                dates,
                amounts,
                action_items,
                ai_analyzed,
                clean_text,
            }));
        }
    }

    Json(None::<MessageDetail>)
}

pub(super) async fn update_message_flags(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(update): Json<FlagUpdate>,
) -> impl IntoResponse {
    if !matches!(update.action.as_str(), "add" | "remove" | "set") {
        return Json(ApiResult {
            success: false,
            message: Some("action must be one of: add, remove, set".into()),
        });
    }

    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store
            .get_message(mb.id, uid)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            let result = match update.action.as_str() {
                "add" => mb_store.add_flags(mb.id, uid, update.flags).await,
                "remove" => mb_store.remove_flags(mb.id, uid, update.flags).await,
                _ => mb_store.update_flags(mb.id, uid, update.flags).await,
            };
            if let Err(e) = &result {
                eprintln!("update_flags error: {e}");
            }
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|_| "failed to update flags".into()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

pub(super) async fn delete_message(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(ApiResult {
            success: false,
            message: Some("mailbox not configured".into()),
        });
    };

    // mark as deleted
    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if mb_store
            .get_message(mb.id, uid)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            let result = mb_store
                .add_flags(mb.id, uid, mailrs_mailbox::FLAG_DELETED)
                .await;
            if let Err(e) = &result {
                eprintln!("delete_message error: {e}");
            }
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|_| "failed to delete message".into()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

/// check if a sender address is allowed for the authenticated user
/// returns Ok(()) if allowed, Err(message) if not
pub(crate) fn verify_sender(
    from: &str,
    user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), &'static str> {
    if from == user {
        return Ok(());
    }
    // check if from is an alias address owned by this user
    if permissions
        .send_as()
        .iter()
        .any(|a| a.eq_ignore_ascii_case(from))
    {
        return Ok(());
    }
    // super user or user with accessible domains
    let accessible = permissions.accessible_domains();
    if !accessible.is_empty() {
        if let Some(domain) = from.rsplit_once('@').map(|(_, d)| d) {
            if permissions.is_super()
                || accessible.iter().any(|sd| sd.eq_ignore_ascii_case(domain))
            {
                return Ok(());
            }
        }
    }
    Err("sender must match authenticated user")
}

/// resolve reply_to_thread_id into in_reply_to message-id and references
/// returns (resolved_in_reply_to, references)
pub(crate) async fn resolve_thread_reply(
    reply_to_thread_id: Option<&str>,
    in_reply_to: Option<&str>,
    user: &str,
    mb_store: Option<&mailrs_mailbox::MailboxStore>,
) -> (Option<String>, Vec<String>) {
    // explicit in_reply_to takes precedence
    if let Some(reply_to) = in_reply_to {
        if !reply_to.is_empty() {
            let refs = match mb_store {
                Some(store) => store
                    .get_thread_references(user, reply_to)
                    .await
                    .unwrap_or_default(),
                None => vec![],
            };
            return (Some(reply_to.to_string()), refs);
        }
    }

    // resolve thread_id to last message's message-id
    if let (Some(thread_id), Some(store)) = (reply_to_thread_id, mb_store) {
        if !thread_id.is_empty() {
            if let Ok(Some(last_msg_id)) = store.get_last_message_id_in_thread(user, thread_id).await {
                let refs = store
                    .get_thread_message_ids(user, thread_id)
                    .await
                    .unwrap_or_default();
                return (Some(last_msg_id), refs);
            }
        }
    }

    (None, vec![])
}

pub(super) async fn send_message(
    AuthUser { address: user, permissions, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
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
    let (final_body, final_html, forwarded_attachments) = if let Some(uid) = req.forward_attachments_from {
        let (orig_text, orig_html, atts) = extract_full_forward(&state, from, uid).await;
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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<SendResult> {
    deliver_message_ex(state, from, to, cc, bcc, raw, message_id, ts, None).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message_ex(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
    scheduled_at: Option<i64>,
) -> Json<SendResult> {
    let all_recipients: Vec<String> = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .map(|s| extract_address(s))
        .collect();

    let local_domains: Vec<String> = if let Some(ref ds) = state.domain_store {
        ds.list_domains()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.name)
            .collect()
    } else {
        vec![]
    };

    let mut errors = Vec::new();

    // resolve group emails to individual members
    let mut resolved_recipients = Vec::new();
    for rcpt in &all_recipients {
        if let Some(ref ds) = state.domain_store {
            match ds.resolve_recipient(rcpt).await {
                crate::domain_store::ResolvedRecipient::Group(members) => {
                    resolved_recipients.extend(members);
                }
                _ => resolved_recipients.push(rcpt.clone()),
            }
        } else {
            resolved_recipients.push(rcpt.clone());
        }
    }

    // deduplicate recipients (e.g. user both in a group and directly CC'd)
    resolved_recipients.sort_unstable();
    resolved_recipients.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    for rcpt in &resolved_recipients {
        let domain = rcpt.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let is_local = local_domains
            .iter()
            .any(|d: &String| d.eq_ignore_ascii_case(domain));

        if is_local {
            if let Some(ref mb_store) = state.mailbox_store {
                let _ = mb_store.ensure_default_mailboxes(rcpt).await;
                if let Err(e) = mb_store
                    .append_message(rcpt, "INBOX", &state.maildir_root, raw, 0, ts)
                    .await
                {
                    errors.push(format!("{rcpt}: {e}"));
                }
            }
        } else if let Some(ref pool) = state.outbound_queue {
            let enqueue_result = if let Some(sched) = scheduled_at {
                mailrs_outbound_queue::queue::enqueue_scheduled(
                    pool, from, rcpt, domain, raw, Some(message_id), ts, sched,
                )
                .await
            } else {
                mailrs_outbound_queue::queue::enqueue(
                    pool, from, rcpt, domain, raw, Some(message_id), ts,
                )
                .await
            };
            if let Err(e) = enqueue_result {
                errors.push(format!("{rcpt}: {e}"));
            } else if let Some(ref vk) = state.valkey {
                mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
            }
        } else {
            errors.push(format!("{rcpt}: outbound queue not configured"));
        }
    }

    // save copy to Sent folder
    if let Some(ref mb_store) = state.mailbox_store {
        let _ = mb_store.ensure_default_mailboxes(from).await;
        let _ = mb_store
            .append_message(
                from,
                "Sent",
                &state.maildir_root,
                raw,
                mailrs_mailbox::FLAG_SEEN,
                ts,
            )
            .await;
    }

    if errors.is_empty() {
        Json(SendResult {
            success: true,
            message: None,
            message_id: Some(message_id.to_string()),
        })
    } else {
        Json(SendResult {
            success: false,
            message: Some(errors.join("; ")),
            message_id: None,
        })
    }
}

// extract bare email from "Display Name <addr>" or return as-is
fn extract_address(s: &str) -> String {
    if let Some(start) = s.rfind('<') {
        if let Some(end) = s[start..].find('>') {
            return s[start + 1..start + end].trim().to_string();
        }
    }
    s.trim().to_string()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_rfc5322_message(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    html_body: Option<&str>,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    list_unsubscribe: Option<&str>,
) -> Vec<u8> {
    build_rfc5322_with_attachments(
        from,
        to,
        cc,
        subject,
        body,
        html_body,
        message_id,
        in_reply_to,
        references,
        date,
        &[],
        list_unsubscribe,
        &[],
        false,
    )
}

// build the text/plain + text/html alternative part
fn build_alternative_part(msg: &mut String, text: &str, html: &str) {
    let alt_boundary = format!("----=_Alt_{}", rand_core::OsRng.next_u64());
    msg.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\"\r\n\r\n"
    ));
    // text/plain
    msg.push_str(&format!("--{alt_boundary}\r\n"));
    msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    msg.push_str(text);
    msg.push_str("\r\n");
    // text/html
    msg.push_str(&format!("--{alt_boundary}\r\n"));
    msg.push_str("Content-Type: text/html; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    msg.push_str(html);
    msg.push_str("\r\n");
    msg.push_str(&format!("--{alt_boundary}--\r\n"));
}

/// wrap editor html in a minimal email-safe template with inline styles
pub(super) fn wrap_email_html(html: &str) -> String {
    format!(
        "<!DOCTYPE html>\
<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<style>\
body{{margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;font-size:14px;line-height:1.6;color:#1a1a1a;background:#fff}}\
.wrapper{{max-width:600px;margin:0 auto;padding:16px}}\
pre{{background:#1e1e2e;color:#cdd6f4;padding:12px 16px;border-radius:6px;overflow-x:auto;font-family:'SF Mono',Monaco,Consolas,'Liberation Mono',monospace;font-size:13px;line-height:1.5}}\
code{{font-family:'SF Mono',Monaco,Consolas,'Liberation Mono',monospace;font-size:13px}}\
:not(pre)>code{{background:#f0f0f0;padding:2px 4px;border-radius:3px;font-size:0.9em}}\
blockquote{{border-left:3px solid #d4d4d8;padding-left:12px;margin:8px 0;color:#71717a}}\
img{{max-width:100%;height:auto}}\
table{{border-collapse:collapse;width:100%}}\
th,td{{border:1px solid #d4d4d8;padding:6px 12px;text-align:left}}\
th{{background:#f4f4f5}}\
a{{color:#2563eb}}\
ul[data-type=\"taskList\"]{{list-style:none;padding-left:0}}\
ul[data-type=\"taskList\"] li{{display:flex;align-items:flex-start;gap:4px}}\
h1{{font-size:1.5em}} h2{{font-size:1.3em}} h3{{font-size:1.1em}}\
</style></head><body><div class=\"wrapper\">{html}</div></body></html>"
    )
}

/// extract full body (text + html) and all attachments from an existing message for forwarding
async fn extract_full_forward(
    state: &WebState,
    user: &str,
    uid: u32,
) -> (Option<String>, Option<String>, Vec<AttachmentData>) {
    let empty = (None, None, vec![]);
    let Some(ref mb_store) = state.mailbox_store else { return empty; };
    let Some(meta) = mb_store.find_message_by_uid(user, uid).await.ok().flatten() else {
        eprintln!("forward: message uid={uid} not found for user={user}");
        return empty;
    };
    let Some(raw) = message_util::read_message_raw(&state.maildir_root, user, &meta.maildir_id) else {
        eprintln!("forward: raw message not found for maildir_id={}", meta.maildir_id);
        return empty;
    };

    // use the existing parser that handles nested MIME well
    let (text_body, html_body, _) = message_util::parse_message(&raw);

    // parse attachments from raw MIME
    let mut attachments = Vec::new();
    if let Some(parsed) = mail_parser::MessageParser::default().parse(&raw) {
        for part in parsed.parts.iter().skip(1) {
            let disp = part.content_disposition();
            let is_attachment = disp
                .map(|d| d.ctype() == "attachment" || d.ctype() == "inline")
                .unwrap_or(false);
            let ct = part.content_type();
            let is_body_part = ct
                .map(|c| {
                    let main = c.ctype();
                    let sub = c.subtype().unwrap_or("");
                    (main == "text" && (sub == "plain" || sub == "html")) && !is_attachment
                })
                .unwrap_or(false);
            if is_body_part {
                continue;
            }

            let filename = part
                .attachment_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "attachment".into());
            let content_type = ct
                .map(|c| {
                    let sub = c.subtype().unwrap_or("octet-stream");
                    format!("{}/{}", c.ctype(), sub)
                })
                .unwrap_or_else(|| "application/octet-stream".into());

            let body_bytes = part.contents();
            if !body_bytes.is_empty() {
                attachments.push(AttachmentData {
                    filename,
                    content_type,
                    data: body_bytes.to_vec(),
                });
            }
        }
    }

    eprintln!(
        "forward: uid={uid} text={}bytes html={}bytes attachments={}",
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_rfc5322_with_attachments(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    html_body: Option<&str>,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    attachments: &[AttachmentData],
    list_unsubscribe: Option<&str>,
    inline_images: &[crate::inline_image::InlineImage],
    request_read_receipt: bool,
) -> Vec<u8> {
    let date_str = date.format("%a, %d %b %Y %H:%M:%S %z").to_string();
    let mut msg = format!(
        "Date: {date_str}\r\n\
         From: {from}\r\n\
         To: {}\r\n",
        to.join(", ")
    );
    if !cc.is_empty() {
        msg.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    let encoded_subject = message_util::rfc2047_encode(subject);
    msg.push_str(&format!(
        "Subject: {encoded_subject}\r\n\
         Message-ID: <{message_id}>\r\n\
         MIME-Version: 1.0\r\n"
    ));
    if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("In-Reply-To: <{ref_id}>\r\n"));
    }
    if !references.is_empty() {
        let refs_str = references
            .iter()
            .map(|r| format!("<{r}>"))
            .collect::<Vec<_>>()
            .join(" ");
        msg.push_str(&format!("References: {refs_str}\r\n"));
    } else if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("References: <{ref_id}>\r\n"));
    }
    if let Some(unsub_url) = list_unsubscribe {
        msg.push_str(&format!("List-Unsubscribe: <{unsub_url}>\r\n"));
        msg.push_str("List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n");
    }
    if request_read_receipt {
        msg.push_str(&format!("Disposition-Notification-To: {from}\r\n"));
    }

    // derive full html with email template wrapper
    let wrapped_html = html_body.map(wrap_email_html);
    let has_html = wrapped_html.is_some();

    let has_inline = !inline_images.is_empty();

    // helper: build the "content" part (alternative or related or plain)
    // when inline images exist, wrap alternative in multipart/related
    let build_content_part = |msg: &mut String| {
        if has_html {
            let html = wrapped_html.as_deref().unwrap_or("");
            if has_inline {
                // multipart/related wrapping alternative + inline images
                let rel_boundary = format!("----=_Rel_{}", rand_core::OsRng.next_u64());
                msg.push_str(&format!(
                    "Content-Type: multipart/related; boundary=\"{rel_boundary}\"\r\n\r\n"
                ));
                msg.push_str(&format!("--{rel_boundary}\r\n"));
                build_alternative_part(msg, body, html);
                msg.push_str(&crate::inline_image::build_inline_parts(
                    inline_images,
                    &rel_boundary,
                ));
                msg.push_str(&format!("--{rel_boundary}--\r\n"));
            } else {
                build_alternative_part(msg, body, html);
            }
        } else {
            msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
            msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
            msg.push_str(body);
            msg.push_str("\r\n");
        }
    };

    if attachments.is_empty() {
        build_content_part(&mut msg);
    } else {
        let boundary = format!("----=_Part_{}", rand_core::OsRng.next_u64());
        msg.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
        ));

        msg.push_str(&format!("--{boundary}\r\n"));
        build_content_part(&mut msg);

        // attachment parts
        for att in attachments {
            msg.push_str(&format!("--{boundary}\r\n"));
            let name_param = message_util::rfc2231_encode_param("name", &att.filename);
            msg.push_str(&format!(
                "Content-Type: {}; {name_param}\r\n",
                att.content_type
            ));
            msg.push_str("Content-Transfer-Encoding: base64\r\n");
            let filename_param = message_util::rfc2231_encode_param("filename", &att.filename);
            msg.push_str(&format!(
                "Content-Disposition: attachment; {filename_param}\r\n\r\n",
            ));

            let encoded = base64::engine::general_purpose::STANDARD.encode(&att.data);
            for chunk in encoded.as_bytes().chunks(76) {
                msg.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                msg.push_str("\r\n");
            }
        }

        msg.push_str(&format!("--{boundary}--\r\n"));
    }

    msg.into_bytes()
}

pub(super) async fn send_message_multipart(
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

pub(super) async fn get_message_raw(
    Path(uid): Path<u32>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::NOT_FOUND,
            [
                ("content-type", "text/plain".to_string()),
                ("content-disposition", String::new()),
            ],
            b"mailbox not configured".to_vec(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            if let Some(data) = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id) {
                let subject = message_util::decode_header(&msg.subject);
                let safe_name = subject
                    .chars()
                    .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
                    .collect::<String>();
                let filename = if safe_name.trim().is_empty() {
                    format!("message-{uid}.eml")
                } else {
                    format!("{}.eml", safe_name.trim())
                };
                let disposition = format!("attachment; filename=\"{filename}\"");
                return (
                    StatusCode::OK,
                    [
                        ("content-type", "message/rfc822".to_string()),
                        ("content-disposition", disposition),
                    ],
                    data,
                );
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/plain".to_string()),
            ("content-disposition", String::new()),
        ],
        b"message not found".to_vec(),
    )
}

pub(super) async fn get_attachment(
    Path((uid, index)): Path<(u32, usize)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::NOT_FOUND,
            [
                ("content-type", "text/plain".to_string()),
                ("content-disposition", String::new()),
            ],
            Vec::new(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    for mb in &mailboxes {
        if let Ok(Some(msg)) = mb_store.get_message(mb.id, uid).await {
            let raw = message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id);
            if let Some(data) = raw {
                if let Some(parsed) = mail_parser::MessageParser::default().parse(&data) {
                    let attachments: Vec<_> = parsed.attachments().collect();
                    if let Some(att) = attachments.get(index) {
                        let filename = att
                            .attachment_name()
                            .or_else(|| att.content_type().and_then(|ct| ct.attribute("name")))
                            .unwrap_or("unnamed")
                            .to_string();
                        let content_type = att
                            .content_type()
                            .map(|ct| {
                                if let Some(sub) = ct.subtype() {
                                    format!("{}/{}", ct.ctype(), sub)
                                } else {
                                    ct.ctype().to_string()
                                }
                            })
                            .unwrap_or_else(|| "application/octet-stream".into());
                        let body = att.contents().to_vec();

                        // use inline for browser-viewable types, attachment for the rest
                        let inline = content_type.starts_with("image/")
                            || content_type.starts_with("text/")
                            || content_type == "application/pdf";
                        let param = message_util::rfc2231_encode_param("filename", &filename);
                        let disposition = if inline {
                            format!("inline; {param}")
                        } else {
                            format!("attachment; {param}")
                        };

                        return (
                            StatusCode::OK,
                            [
                                ("content-type", content_type),
                                ("content-disposition", disposition),
                            ],
                            body,
                        );
                    }
                }
            }
        }
    }

    (
        StatusCode::NOT_FOUND,
        [
            ("content-type", "text/plain".to_string()),
            ("content-disposition", String::new()),
        ],
        b"attachment not found".to_vec(),
    )
}

// --- attachment content (OCR/PDF text) ---

#[derive(Serialize)]
struct AttachmentContentResponse {
    success: bool,
    extracted_text: Option<String>,
    language: Option<String>,
    // f64 matches the DOUBLE PRECISION column type
    confidence: f64,
    page_count: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub(super) async fn get_attachment_content(
    Path((uid, index)): Path<(u32, i16)>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("database unavailable".into()),
        });
    };

    // resolve message id and attachment content in a single query, avoiding the
    // N+1 pattern of listing all mailboxes then probing each one for the uid
    let row = sqlx::query_as::<_, (String, Option<String>, f64, Option<i16>)>(
        "SELECT COALESCE(ac.extracted_text, ''), ac.language, ac.ocr_confidence, ac.page_count
         FROM attachment_content ac
         JOIN messages m ON ac.message_id = m.id
         JOIN mailboxes mb ON m.mailbox_id = mb.id
         WHERE mb.user_address = $1 AND m.uid = $2 AND ac.attachment_index = $3
         LIMIT 1",
    )
    .bind(&user)
    .bind(uid as i32)
    .bind(index)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some((text, language, confidence, page_count))) => Json(AttachmentContentResponse {
            success: true,
            extracted_text: Some(text),
            language,
            confidence,
            page_count,
            message: None,
        }),
        Ok(None) => Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("content not yet extracted".into()),
        }),
        Err(_) => Json(AttachmentContentResponse {
            success: false,
            extracted_text: None,
            language: None,
            confidence: 0.0,
            page_count: None,
            message: Some("internal error".into()),
        }),
    }
}

// --- inline image handlers ---

#[derive(Serialize)]
struct InlineUploadResult {
    success: bool,
    id: Option<String>,
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub(super) async fn upload_inline_image(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut image_data: Option<Vec<u8>> = None;
    let mut content_type = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            content_type = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();
            if let Ok(data) = field.bytes().await {
                image_data = Some(data.to_vec());
            }
        }
    }

    let Some(data) = image_data else {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some("no image field provided".into()),
        });
    };

    if let Err(e) = crate::inline_image::validate_inline_upload(&data, &content_type) {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some(e),
        });
    }

    let id = crate::inline_image::generate_inline_id();
    let ext = crate::inline_image::ext_from_content_type(&content_type);
    let path = crate::inline_image::inline_path(&state.maildir_root, &user, &id, ext);

    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Json(InlineUploadResult {
                success: false,
                id: None,
                url: None,
                message: Some(format!("create dir: {e}")),
            });
        }
    }

    if let Err(e) = tokio::fs::write(&path, &data).await {
        return Json(InlineUploadResult {
            success: false,
            id: None,
            url: None,
            message: Some(format!("write file: {e}")),
        });
    }

    let url = format!("/api/mail/inline/{id}");
    Json(InlineUploadResult {
        success: true,
        id: Some(id),
        url: Some(url),
        message: None,
    })
}

pub(super) async fn serve_inline_image(
    Path(id): Path<String>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    // validate ID format: only permit strictly safe alphanumeric+underscore IDs
    if !crate::inline_image::is_valid_inline_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid id").into_response();
    }

    // try each known extension for exact file match (avoids prefix collision)
    let known_exts = ["png", "jpg", "webp", "gif", "tiff", "bmp", "svg", "bin"];
    let mut found = None;
    for ext in &known_exts {
        let path = crate::inline_image::inline_path(&state.maildir_root, &user, &id, ext);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            found = Some(path);
            break;
        }
    }

    let Some(file_path) = found else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };

    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let content_type = match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    match tokio::fs::read(&file_path).await {
        Ok(data) => {
            let mut resp = (StatusCode::OK, data).into_response();
            let h = resp.headers_mut();
            h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
            h.insert(header::CONTENT_DISPOSITION, "inline".parse().unwrap());
            resp
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response(),
    }
}

// --- draft handlers ---

pub(super) async fn save_draft(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SaveDraftRequest>,
) -> impl IntoResponse {
    if req.body.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("draft body too large".into()),
        });
    }
    if req.subject.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("subject too long".into()),
        });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("database not configured".into()),
        });
    };

    let result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO drafts (user_address, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id",
    )
    .bind(&user)
    .bind(&req.to)
    .bind(&req.cc)
    .bind(&req.bcc)
    .bind(&req.subject)
    .bind(&req.body)
    .bind(&req.reply_to_thread_id)
    .fetch_one(pool)
    .await;

    match result {
        Ok(id) => Json(SaveDraftResult {
            success: true,
            id: Some(id),
            message: None,
        }),
        Err(e) => {
            eprintln!("save_draft db error: {e}");
            Json(SaveDraftResult {
                success: false,
                id: None,
                message: Some("failed to save draft".into()),
            })
        }
    }
}

pub(super) async fn list_drafts(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<DraftInfo>::new());
    };

    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id, created_at, updated_at \
         FROM drafts WHERE user_address = $1 ORDER BY updated_at DESC",
    )
    .bind(&user)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let drafts: Vec<DraftInfo> = rows
        .into_iter()
        .map(|(id, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id, created_at, updated_at)| {
            DraftInfo {
                id,
                to_addresses,
                cc_addresses,
                bcc_addresses,
                subject,
                body,
                reply_to_thread_id,
                created_at: created_at.to_rfc3339(),
                updated_at: updated_at.to_rfc3339(),
            }
        })
        .collect();

    Json(drafts)
}

pub(super) async fn delete_draft(
    Path(id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database not configured".into()),
        });
    };

    let result = sqlx::query("DELETE FROM drafts WHERE id = $1 AND user_address = $2")
        .bind(id)
        .bind(&user)
        .execute(pool)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(_) => Json(ApiResult {
            success: false,
            message: Some("draft not found".into()),
        }),
        Err(e) => {
            eprintln!("delete_draft db error: {e}");
            Json(ApiResult {
                success: false,
                message: Some("failed to delete draft".into()),
            })
        }
    }
}

pub(super) async fn export_mbox(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref mb_store) = state.mailbox_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [
                ("content-type", "text/plain".to_string()),
                ("content-disposition", String::new()),
            ],
            b"mailbox not configured".to_vec(),
        );
    };

    let mailboxes = mb_store.list_mailboxes(&user).await.unwrap_or_default();
    let mut mbox = Vec::new();

    for mb in &mailboxes {
        let (total, _) = mb_store.mailbox_status(mb.id).await.unwrap_or((0, 0));
        let mut offset = 0u32;
        let page_size = 100u32;
        while offset < total {
            let messages = mb_store
                .list_messages(mb.id, offset, page_size)
                .await
                .unwrap_or_default();
            if messages.is_empty() {
                break;
            }
            for msg in &messages {
                if let Some(raw) = message_util::read_message_raw(
                    &state.maildir_root,
                    &user,
                    &msg.maildir_id,
                ) {
                    // mbox "From " line: use sender and date_epoch
                    let sender = msg.sender.trim();
                    let sender_addr = extract_address(sender);
                    let datetime = chrono::DateTime::from_timestamp(msg.date, 0)
                        .unwrap_or_default();
                    let from_line = format!(
                        "From {} {}\n",
                        sender_addr,
                        datetime.format("%a %b %d %H:%M:%S %Y"),
                    );
                    mbox.extend_from_slice(from_line.as_bytes());
                    mbox.extend_from_slice(&raw);
                    if !raw.ends_with(b"\n") {
                        mbox.push(b'\n');
                    }
                    mbox.push(b'\n');
                }
            }
            offset += page_size;
        }
    }

    (
        StatusCode::OK,
        [
            ("content-type", "application/mbox".to_string()),
            (
                "content-disposition",
                "attachment; filename=\"mailbox.mbox\"".to_string(),
            ),
        ],
        mbox,
    )
}

// --- signatures ---

#[derive(Serialize)]
pub(super) struct SignatureInfo {
    pub id: i64,
    pub name: String,
    pub html: String,
    pub text_content: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Deserialize)]
pub(super) struct SaveSignatureRequest {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default = "default_signature_name")]
    pub name: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text_content: String,
    #[serde(default)]
    pub is_default: bool,
}

fn default_signature_name() -> String {
    "Default".into()
}

pub(super) async fn list_signatures(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(serde_json::json!([]));
    };
    let rows = sqlx::query_as::<_, (i64, String, String, String, bool, String)>(
        "SELECT id, name, html, text_content, is_default, \
         to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM signatures WHERE account_address = $1 ORDER BY created_at",
    )
    .bind(address)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let sigs: Vec<SignatureInfo> = rows
        .into_iter()
        .map(|(id, name, html, text_content, is_default, created_at)| SignatureInfo {
            id,
            name,
            html,
            text_content,
            is_default,
            created_at,
        })
        .collect();
    Json(serde_json::to_value(sigs).unwrap_or_default())
}

pub(super) async fn save_signature(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SaveSignatureRequest>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database unavailable".into()),
        });
    };
    if req.name.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("signature name too long".into()),
        });
    }
    if req.html.len() > super::MAX_EMAIL_BODY_LEN || req.text_content.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("signature content too long".into()),
        });
    }

    // if setting as default, unset any existing default first
    if req.is_default {
        let _ = sqlx::query(
            "UPDATE signatures SET is_default = false WHERE account_address = $1",
        )
        .bind(address)
        .execute(pool)
        .await;
    }

    let result = if let Some(id) = req.id {
        // update existing
        sqlx::query(
            "UPDATE signatures SET name = $1, html = $2, text_content = $3, is_default = $4 \
             WHERE id = $5 AND account_address = $6",
        )
        .bind(&req.name)
        .bind(&req.html)
        .bind(&req.text_content)
        .bind(req.is_default)
        .bind(id)
        .bind(address)
        .execute(pool)
        .await
    } else {
        // insert new
        sqlx::query(
            "INSERT INTO signatures (account_address, name, html, text_content, is_default) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(address)
        .bind(&req.name)
        .bind(&req.html)
        .bind(&req.text_content)
        .bind(req.is_default)
        .execute(pool)
        .await
    };

    match result {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn delete_signature(
    Path(id): Path<i64>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database unavailable".into()),
        });
    };
    let result = sqlx::query(
        "DELETE FROM signatures WHERE id = $1 AND account_address = $2",
    )
    .bind(id)
    .bind(address)
    .execute(pool)
    .await;
    match result {
        Ok(r) if r.rows_affected() > 0 => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(_) => Json(ApiResult {
            success: false,
            message: Some("signature not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

/// cancel a pending outbound message (undo send)
pub(super) async fn cancel_pending_send(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Path(message_id): Path<String>,
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

// --- encryption keys ---

/// maximum length for PGP/S/MIME public key data
const MAX_PUBLIC_KEY_LEN: usize = 256 * 1024;

#[derive(Serialize)]
pub(super) struct EncryptionKeyInfo {
    pub key_type: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub(super) struct SetKeyRequest {
    pub public_key: String,
    #[serde(default)]
    pub fingerprint: String,
}

fn validate_key_type(key_type: &str) -> bool {
    key_type == "pgp" || key_type == "smime"
}

pub(super) async fn list_keys(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let rows = ds.list_encryption_keys(address).await.unwrap_or_default();
    let items: Vec<EncryptionKeyInfo> = rows
        .into_iter()
        .map(|(_, key_type, fingerprint, created_at)| EncryptionKeyInfo {
            key_type,
            fingerprint,
            created_at,
        })
        .collect();
    Json(serde_json::to_value(items).unwrap_or_default())
}

pub(super) async fn get_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid key_type, must be pgp or smime"})),
        );
    }
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "storage unavailable"})),
        );
    };
    match ds.get_encryption_key(address, &key_type).await {
        Ok(Some((_id, public_key, fingerprint))) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key_type": key_type,
                "public_key": public_key,
                "fingerprint": fingerprint,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

pub(super) async fn set_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetKeyRequest>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return Json(ApiResult {
            success: false,
            message: Some("invalid key_type, must be pgp or smime".into()),
        });
    }
    if req.public_key.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("public_key is required".into()),
        });
    }
    if req.public_key.len() > MAX_PUBLIC_KEY_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("public_key too large".into()),
        });
    }
    if req.fingerprint.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("fingerprint too long".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("storage unavailable".into()),
        });
    };
    match ds
        .set_encryption_key(address, &key_type, &req.public_key, &req.fingerprint)
        .await
    {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn delete_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return Json(ApiResult {
            success: false,
            message: Some("invalid key_type, must be pgp or smime".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("storage unavailable".into()),
        });
    };
    match ds.delete_encryption_key(address, &key_type).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("key not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

/// public endpoint: look up anyone's PGP public key by address
pub(super) async fn get_public_pgp_key(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    get_public_key_inner(&address, "pgp", &state).await
}

/// public endpoint: look up anyone's S/MIME certificate by address
pub(super) async fn get_public_smime_key(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    get_public_key_inner(&address, "smime", &state).await
}

async fn get_public_key_inner(
    address: &str,
    key_type: &str,
    state: &WebState,
) -> (StatusCode, Json<serde_json::Value>) {
    if address.len() > super::MAX_ADMIN_FIELD_LEN || !address.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid address"})),
        );
    }
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "storage unavailable"})),
        );
    };
    match ds.get_encryption_key(address, key_type).await {
        Ok(Some((_id, public_key, fingerprint))) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "address": address,
                "key_type": key_type,
                "public_key": public_key,
                "fingerprint": fingerprint,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

/// look up BIMI logo URL for a domain (cached in Valkey for 24h)
pub(super) async fn get_bimi_logo(
    Path(domain): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    // validate domain
    if domain.len() > 253 || domain.contains('/') || !domain.contains('.') {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid domain"})));
    }

    // check valkey cache
    let cache_key = format!("bimi:{domain}");
    if let Some(mut conn) = state.valkey.clone() {
        if let Ok(Some(cached)) = redis::cmd("GET")
            .arg(&cache_key)
            .query_async::<Option<String>>(&mut conn)
            .await
        {
            if cached.is_empty() {
                return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no BIMI record"})));
            }
            return (StatusCode::OK, Json(serde_json::json!({"logo_url": cached})));
        }
    }

    // dns lookup
    let Some(ref resolver) = state.resolver else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "DNS resolver not available"})));
    };
    let logo_url = crate::domain_check::lookup_bimi_logo(resolver, &domain).await;

    // cache result (24h), empty string = negative cache
    if let Some(mut conn) = state.valkey.clone() {
        let val = logo_url.as_deref().unwrap_or("");
        let _: std::result::Result<(), _> = redis::cmd("SET")
            .arg(&cache_key)
            .arg(val)
            .arg("EX")
            .arg(86400u64)
            .query_async(&mut conn)
            .await;
    }

    match logo_url {
        Some(url) => (StatusCode::OK, Json(serde_json::json!({"logo_url": url}))),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no BIMI record"}))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_address_bare() {
        assert_eq!(extract_address("user@example.com"), "user@example.com");
    }

    #[test]
    fn extract_address_display_name() {
        assert_eq!(
            extract_address("Chenyun Dai <chenyund@qti.qualcomm.com>"),
            "chenyund@qti.qualcomm.com"
        );
    }

    #[test]
    fn extract_address_angle_only() {
        assert_eq!(extract_address("<foo@bar.com>"), "foo@bar.com");
    }

    #[test]
    fn extract_address_with_spaces() {
        assert_eq!(extract_address("  alice@test.org  "), "alice@test.org");
    }

    // --- verify_sender tests ---

    fn make_super_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo, ALL_PERMISSIONS};
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "super".into(),
                domain: None,
                description: String::new(),
                is_builtin: true,
                created_at: 0,
            },
            permissions: ALL_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
        }];
        compute_effective_permissions(
            &groups,
            &[],
            &domains.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    fn make_no_perms() -> crate::permission::EffectivePermissions {
        crate::permission::compute_effective_permissions(&[], &[], &[])
    }

    #[test]
    fn verify_sender_superadmin_matching_domain_allowed() {
        let perms = make_super_perms(&["golia.jp", "example.com"]);
        assert!(verify_sender("agent@golia.jp", "admin@golia.jp", &perms).is_ok());
        // different user but same domain
        assert!(verify_sender("other@example.com", "admin@golia.jp", &perms).is_ok());
    }

    #[test]
    fn verify_sender_superadmin_non_matching_domain_rejected() {
        // super user with only golia.jp domain — but super has all domains, so it should allow
        // let's test with a domain-scoped group instead
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "user".into(),
                domain: Some("golia.jp".into()),
                description: String::new(),
                is_builtin: false,
                created_at: 0,
            },
            permissions: vec!["mail.send".into(), "mail.read".into()],
        }];
        let perms = compute_effective_permissions(&groups, &[], &["golia.jp".into()]);
        assert_eq!(
            verify_sender("agent@evil.com", "admin@golia.jp", &perms),
            Err("sender must match authenticated user")
        );
    }

    #[test]
    fn verify_sender_non_superadmin_different_from_rejected() {
        let perms = make_no_perms();
        assert_eq!(
            verify_sender("other@golia.jp", "user@golia.jp", &perms),
            Err("sender must match authenticated user")
        );
    }

    #[test]
    fn verify_sender_non_superadmin_matching_from_allowed() {
        let perms = make_no_perms();
        assert!(verify_sender("user@golia.jp", "user@golia.jp", &perms).is_ok());
    }

    // --- resolve_thread_reply tests ---

    #[tokio::test]
    async fn resolve_thread_reply_thread_id_resolves_when_no_in_reply_to() {
        // when no mailbox store and no in_reply_to, thread_id cannot resolve (no DB)
        // but it should not panic
        let (reply, refs) = resolve_thread_reply(
            Some("thread-abc"),
            None,
            "user@test.com",
            None,
        ).await;
        // without a store, cannot resolve thread_id
        assert!(reply.is_none());
        assert!(refs.is_empty());
    }

    #[tokio::test]
    async fn resolve_thread_reply_explicit_in_reply_to_takes_precedence() {
        // explicit in_reply_to should be used even if reply_to_thread_id is present
        let (reply, _refs) = resolve_thread_reply(
            Some("thread-abc"),
            Some("explicit-msg-id@test.com"),
            "user@test.com",
            None,
        ).await;
        assert_eq!(reply.as_deref(), Some("explicit-msg-id@test.com"));
    }

    // --- validate_key_type tests ---

    #[test]
    fn validate_key_type_pgp() {
        assert!(validate_key_type("pgp"));
    }

    #[test]
    fn validate_key_type_smime() {
        assert!(validate_key_type("smime"));
    }

    #[test]
    fn validate_key_type_invalid() {
        assert!(!validate_key_type("rsa"));
        assert!(!validate_key_type(""));
        assert!(!validate_key_type("PGP"));
    }
}

// --- deliverability check ---

#[derive(Deserialize)]
pub(super) struct DeliverabilityCheckRequest {
    pub recipient: String,
}

#[derive(Serialize)]
pub(super) struct DeliverabilityCheckResult {
    pub recipient: String,
    pub suppressed: bool,
    pub mx_found: bool,
    pub mx_hosts: Vec<String>,
    pub issues: Vec<String>,
}

pub(super) async fn check_deliverability(
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

// --- image proxy ---

const IMAGE_PROXY_MAX_BYTES: usize = 5 * 1024 * 1024; // 5 MB
const IMAGE_PROXY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Deserialize)]
pub(super) struct ImageProxyQuery {
    pub url: String,
}

pub(super) async fn proxy_image(
    _auth: AuthUser,
    Query(q): Query<ImageProxyQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    let url = &q.url;

    // only allow http/https
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return (StatusCode::BAD_REQUEST, "invalid url scheme").into_response();
    }

    // check valkey cache first
    if let Some(ref valkey) = state.valkey {
        let cache_key = format!("imgproxy:{}", url);
        {
            if let Ok(cached) = redis::cmd("GET")
                .arg(&cache_key)
                .query_async::<Vec<u8>>(&mut valkey.clone())
                .await
            {
                if !cached.is_empty() {
                    // first byte stores content-type length, then content-type, then image data
                    let ct_len = cached[0] as usize;
                    if cached.len() > 1 + ct_len {
                        let ct = String::from_utf8_lossy(&cached[1..1 + ct_len]).to_string();
                        let body = cached[1 + ct_len..].to_vec();
                        return (
                            StatusCode::OK,
                            [
                                (header::CONTENT_TYPE, ct),
                                (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
                            ],
                            body,
                        )
                            .into_response();
                    }
                }
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(IMAGE_PROXY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    let resp = match client
        .get(url)
        .header("User-Agent", "mailrs/image-proxy")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_GATEWAY, "fetch failed").into_response(),
    };

    if !resp.status().is_success() {
        return (StatusCode::BAD_GATEWAY, "upstream error").into_response();
    }

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // reject non-image responses
    if !content_type.starts_with("image/") {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "not an image").into_response();
    }

    let body = match resp.bytes().await {
        Ok(b) if b.len() <= IMAGE_PROXY_MAX_BYTES => b.to_vec(),
        Ok(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "image too large").into_response(),
        Err(_) => return (StatusCode::BAD_GATEWAY, "read failed").into_response(),
    };

    // cache in valkey (1 hour TTL)
    if let Some(ref valkey) = state.valkey {
        let cache_key = format!("imgproxy:{}", url);
        let ct_bytes = content_type.as_bytes();
        if ct_bytes.len() < 256 {
            let mut packed = Vec::with_capacity(1 + ct_bytes.len() + body.len());
            packed.push(ct_bytes.len() as u8);
            packed.extend_from_slice(ct_bytes);
            packed.extend_from_slice(&body);
            let _ = redis::cmd("SET")
                .arg(&cache_key)
                .arg(&packed)
                .arg("EX")
                .arg(3600i64)
                .query_async::<()>(&mut valkey.clone())
                .await;
        }
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=86400".to_string()),
        ],
        body,
    )
        .into_response()
}

// --- link protection proxy ---

/// known phishing / malicious URL patterns
const BLOCKED_DOMAINS: &[&str] = &[
    // placeholder — extend with real blocklist or external API
];

#[derive(Deserialize)]
pub(super) struct LinkProxyQuery {
    pub url: String,
}

/// check whether a URL should be blocked
fn is_url_blocked(url: &str) -> bool {
    // extract host from URL
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|s| s.split('/').next())
        .and_then(|s| s.split('?').next())
        .and_then(|s| s.split(':').next())
        .unwrap_or("");

    for blocked in BLOCKED_DOMAINS {
        if host == *blocked || host.ends_with(&format!(".{blocked}")) {
            return true;
        }
    }

    // block suspicious patterns
    if url.contains("@") && url.contains("http") {
        // e.g. http://legit.com@evil.com
        return true;
    }

    false
}

pub(super) async fn proxy_link(
    _auth: AuthUser,
    Query(q): Query<LinkProxyQuery>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    use axum::http::header;

    let url = &q.url;

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return (StatusCode::BAD_REQUEST, "invalid url scheme").into_response();
    }

    // check valkey blocklist cache
    if let Some(ref valkey) = state.valkey {
        let cache_key = format!("linkblock:{}", url);
        if let Ok(blocked) = redis::cmd("GET")
            .arg(&cache_key)
            .query_async::<Option<String>>(&mut valkey.clone())
            .await
        {
            if blocked.as_deref() == Some("1") {
                return link_warning_page(url).into_response();
            }
        }
    }

    if is_url_blocked(url) {
        // cache the block decision
        if let Some(ref valkey) = state.valkey {
            let cache_key = format!("linkblock:{}", url);
            let _ = redis::cmd("SET")
                .arg(&cache_key)
                .arg("1")
                .arg("EX")
                .arg(86400i64)
                .query_async::<()>(&mut valkey.clone())
                .await;
        }
        return link_warning_page(url).into_response();
    }

    // record click (fire-and-forget to valkey)
    if let Some(ref valkey) = state.valkey {
        let host = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .unwrap_or("unknown");
        let counter_key = format!("linkclick:{host}");
        let _ = redis::cmd("INCR")
            .arg(&counter_key)
            .query_async::<i64>(&mut valkey.clone())
            .await;
    }

    // safe — redirect
    (StatusCode::FOUND, [(header::LOCATION, url.to_string())]).into_response()
}

fn link_warning_page(url: &str) -> impl IntoResponse {
    use axum::http::header;

    let escaped = url.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Link Warning</title>
<style>
body {{ font-family: -apple-system, sans-serif; max-width: 600px; margin: 80px auto; padding: 20px; color: #1a1a1a; }}
.warn {{ background: #fef2f2; border: 1px solid #fca5a5; border-radius: 8px; padding: 24px; }}
h1 {{ color: #dc2626; font-size: 20px; margin: 0 0 12px; }}
p {{ margin: 8px 0; line-height: 1.6; }}
code {{ background: #f5f5f5; padding: 2px 6px; border-radius: 4px; word-break: break-all; font-size: 13px; }}
.actions {{ margin-top: 20px; display: flex; gap: 12px; }}
a.btn {{ display: inline-block; padding: 8px 20px; border-radius: 6px; text-decoration: none; font-weight: 500; font-size: 14px; }}
a.back {{ background: #2563eb; color: white; }}
a.proceed {{ background: #e5e7eb; color: #374151; }}
</style></head><body>
<div class="warn">
<h1>⚠ Suspicious Link Detected</h1>
<p>This link may be unsafe:</p>
<p><code>{escaped}</code></p>
<p>It matched a known malicious pattern. If you trust this link, you can proceed at your own risk.</p>
<div class="actions">
<a class="btn back" href="javascript:history.back()">Go Back</a>
<a class="btn proceed" href="{escaped}" rel="noopener noreferrer">Proceed Anyway</a>
</div></div></body></html>"#
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())],
        html,
    )
}
