//! Mailbox folder listing + mbox export.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::message_util;

use super::{AuthUser, WebState};

#[derive(Serialize)]
pub(crate) struct FolderInfo {
    pub name: String,
    pub total: u32,
    pub unseen: u32,
    pub uidnext: u32,
}

#[derive(Serialize)]
pub(crate) struct MessageSummary {
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    pub subject: String,
    pub size: u32,
    pub flags: u32,
    pub internal_date: i64,
}

#[derive(Deserialize)]
pub(crate) struct FolderMessagesQuery {
    #[serde(default = "super::default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

pub(crate) async fn get_folders(
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

pub(crate) async fn get_folder_messages(
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

// wire shape mirrors fastcore's `SentMessageSummary` (uid/message_id/
// thread_id/to/subject/internal_date) so the frontend parses both lanes
// identically — core-mode parity.
#[derive(Serialize)]
pub(crate) struct SentMessageSummary {
    pub uid: u32,
    pub message_id: String,
    pub thread_id: String,
    pub to: String,
    pub subject: String,
    pub internal_date: i64,
}

/// GET /api/mail/sent — one row per outbound message (not per thread),
/// newest first, each carrying the recipient (To) + thread_id + uid.
pub(crate) async fn list_sent_messages(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<SentMessageSummary>::new());
    };
    let Some(ref mb_store) = state.mailbox_store else {
        return Json(Vec::<SentMessageSummary>::new());
    };
    let mb = match mb_store.get_mailbox(&user, "Sent").await {
        Ok(Some(mb)) => mb,
        _ => return Json(Vec::<SentMessageSummary>::new()),
    };
    let rows = sqlx::query_as::<_, (i32, String, String, String, String, i64)>(
        "SELECT uid, message_id, thread_id, recipients, subject, internal_date \
         FROM messages WHERE mailbox_id = $1 ORDER BY internal_date DESC LIMIT 500",
    )
    .bind(mb.id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let out: Vec<SentMessageSummary> = rows
        .into_iter()
        .map(
            |(uid, message_id, thread_id, recipients, subject, internal_date)| SentMessageSummary {
                uid: uid as u32,
                message_id,
                thread_id,
                to: recipients,
                subject: message_util::decode_header(&subject),
                internal_date,
            },
        )
        .collect();
    Json(out)
}

pub(crate) async fn export_mbox(
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
                if let Some(raw) =
                    message_util::read_message_raw(&state.maildir_root, &user, &msg.maildir_id)
                        .await
                {
                    // mbox "From " line: use sender and date_epoch
                    let sender = msg.sender.trim();
                    let sender_addr = super::common::extract_address(sender);
                    let datetime =
                        chrono::DateTime::from_timestamp(msg.date, 0).unwrap_or_default();
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
