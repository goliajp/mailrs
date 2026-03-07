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

use super::{ApiResult, AuthUser, WebState};

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
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub list_unsubscribe: Option<String>,
}

pub(super) struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub(super) async fn get_folders(
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
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
    AuthUser(user): AuthUser,
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
            return Json(ApiResult {
                success: result.is_ok(),
                message: result.err().map(|e| e.to_string()),
            });
        }
    }

    Json(ApiResult {
        success: false,
        message: Some("message not found".into()),
    })
}

pub(super) async fn send_message(
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    if req.to.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("to is required".into()),
        });
    }

    let total_recipients = req.to.len() + req.cc.len() + req.bcc.len();
    if total_recipients > super::MAX_RECIPIENTS {
        return Json(ApiResult {
            success: false,
            message: Some(format!("too many recipients (max {})", super::MAX_RECIPIENTS)),
        });
    }

    if req.body.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("message body too large".into()),
        });
    }

    if req.subject.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("subject too long".into()),
        });
    }

    // use authenticated user as sender
    let from = if req.from.is_empty() {
        &user
    } else {
        &req.from
    };

    // verify sender matches authenticated user
    if from != &user {
        return Json(ApiResult {
            success: false,
            message: Some("sender must match authenticated user".into()),
        });
    }

    let now = chrono::Utc::now();
    let message_id = format!(
        "{}.{}@{}",
        now.timestamp_millis(),
        rand_core::OsRng.next_u32(),
        state.hostname
    );

    // build full References chain from thread history
    let references = match (req.in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => mb_store
            .get_thread_references(from, reply_to)
            .await
            .unwrap_or_default(),
        _ => vec![],
    };

    // append quoted text from original message for replies
    let body_with_quote = match (req.in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
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

    let raw = build_rfc5322_message(
        from,
        &req.to,
        &req.cc,
        &req.subject,
        &body_with_quote,
        &message_id,
        req.in_reply_to.as_deref(),
        &references,
        &now,
        req.list_unsubscribe.as_deref(),
    );

    deliver_message(
        &state,
        from,
        &req.to,
        &req.cc,
        &req.bcc,
        &raw,
        &message_id,
        now.timestamp(),
    )
    .await
}

pub(super) async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<ApiResult> {
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

    for rcpt in &all_recipients {
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
            if let Err(e) = mailrs_outbound_queue::queue::enqueue(
                pool,
                from,
                rcpt,
                domain,
                raw,
                Some(message_id),
                ts,
            )
            .await
            {
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
        Json(ApiResult {
            success: true,
            message: None,
        })
    } else {
        Json(ApiResult {
            success: false,
            message: Some(errors.join("; ")),
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

pub(super) fn build_rfc5322_message(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
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
        message_id,
        in_reply_to,
        references,
        date,
        &[],
        list_unsubscribe,
    )
}

pub(super) fn build_rfc5322_with_attachments(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    attachments: &[AttachmentData],
    list_unsubscribe: Option<&str>,
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

    if attachments.is_empty() {
        msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        msg.push_str("Content-Transfer-Encoding: 8bit\r\n");
        msg.push_str("\r\n");
        msg.push_str(body);
    } else {
        let boundary = format!("----=_Part_{}", rand_core::OsRng.next_u64());
        msg.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
        ));

        // text part
        msg.push_str(&format!("--{boundary}\r\n"));
        msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
        msg.push_str(body);
        msg.push_str("\r\n");

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
            // wrap at 76 chars per RFC 2045
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
    AuthUser(user): AuthUser,
    State(state): State<Arc<WebState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut from = String::new();
    let mut to: Vec<String> = Vec::new();
    let mut cc: Vec<String> = Vec::new();
    let mut subject = String::new();
    let mut body = String::new();
    let mut in_reply_to: Option<String> = None;
    let mut attachments: Vec<AttachmentData> = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "from" => from = field.text().await.unwrap_or_default(),
            "to" => to.push(field.text().await.unwrap_or_default()),
            "cc" => cc.push(field.text().await.unwrap_or_default()),
            "subject" => subject = field.text().await.unwrap_or_default(),
            "body" => body = field.text().await.unwrap_or_default(),
            "in_reply_to" => {
                let val = field.text().await.unwrap_or_default();
                if !val.is_empty() {
                    in_reply_to = Some(val);
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

    if from != user {
        return Json(ApiResult {
            success: false,
            message: Some("sender must match authenticated user".into()),
        });
    }

    if to.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("to is required".into()),
        });
    }

    let total_recipients = to.len() + cc.len();
    if total_recipients > super::MAX_RECIPIENTS {
        return Json(ApiResult {
            success: false,
            message: Some(format!("too many recipients (max {})", super::MAX_RECIPIENTS)),
        });
    }

    if body.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("message body too large".into()),
        });
    }

    let now = chrono::Utc::now();
    let message_id = format!(
        "{}.{}@{}",
        now.timestamp_millis(),
        rand_core::OsRng.next_u32(),
        state.hostname
    );

    // build full References chain from thread history
    let references = match (in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
        (Some(reply_to), Some(mb_store)) if !reply_to.is_empty() => mb_store
            .get_thread_references(&from, reply_to)
            .await
            .unwrap_or_default(),
        _ => vec![],
    };

    // append quoted text from original message for replies
    let body_with_quote = match (in_reply_to.as_deref(), state.mailbox_store.as_ref()) {
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

    let raw = build_rfc5322_with_attachments(
        &from,
        &to,
        &cc,
        &subject,
        &body_with_quote,
        &message_id,
        in_reply_to.as_deref(),
        &references,
        &now,
        &attachments,
        None,
    );

    deliver_message(
        &state,
        &from,
        &to,
        &cc,
        &[],
        &raw,
        &message_id,
        now.timestamp(),
    )
    .await
}

pub(super) async fn get_message_raw(
    Path(uid): Path<u32>,
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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

                        // use inline for images so browsers can preview them
                        let disposition = if content_type.starts_with("image/") {
                            "inline".to_string()
                        } else {
                            let param = message_util::rfc2231_encode_param("filename", &filename);
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

// --- draft handlers ---

pub(super) async fn save_draft(
    AuthUser(user): AuthUser,
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
        Err(e) => Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some(e.to_string()),
        }),
    }
}

pub(super) async fn list_drafts(
    AuthUser(user): AuthUser,
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
    AuthUser(user): AuthUser,
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
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
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
}
