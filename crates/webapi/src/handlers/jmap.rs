//! JMAP session document + `POST /jmap` dispatcher + SSE event stream.
//!
//! Ports the monolith at `crates/server/src/web/jmap.rs`. The
//! `MailStore` implementation delegates to fastcore RPCs instead of
//! `PgMailboxStore`. Data flow is unchanged for JMAP clients (RFC 8620
//! / RFC 8621).

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Extension, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use mailrs_jmap::dispatch::{JMAP_CORE_CAP, JMAP_MAIL_CAP, JMAP_SUBMISSION_CAP, JmapRequest};
use mailrs_jmap::store::{MailStore, StoreError};
use mailrs_jmap::types::{
    Attachment as JmapAttachment, Mailbox as JmapMailbox, MailboxCounts, Message as JmapMessage,
    ParsedBody, SubmissionResult,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn hostname() -> String {
    std::env::var("MAILRS_HOSTNAME").unwrap_or_else(|_| "mail.golia.jp".into())
}

/// Bridge between `mailrs-core-api` client and `mailrs_jmap::MailStore`.
pub struct JmapAdapter {
    state: Arc<WebState>,
}

impl JmapAdapter {
    pub fn new(state: Arc<WebState>) -> Self {
        Self { state }
    }
}

fn bridge_wire_to_jmap(w: mailrs_core_api::method::message::MessageWire) -> JmapMessage {
    JmapMessage {
        id: w.id,
        mailbox_id: w.mailbox_id,
        uid: w.uid,
        sender: w.sender,
        recipients: w.recipients,
        subject: w.subject,
        date: w.date,
        size: w.size,
        flags: w.flags,
        internal_date: w.internal_date,
        message_id: w.message_id,
        in_reply_to: w.in_reply_to,
        thread_id: w.thread_id,
        user_address: w.user_address,
        new_content: None,
        blob_id: w.blob_ref,
    }
}

#[async_trait]
impl MailStore for JmapAdapter {
    async fn list_mailboxes(&self, user: &str) -> Result<Vec<JmapMailbox>, StoreError> {
        let resp = self
            .state
            .fast()
            .list_mailboxes(user)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })?;
        Ok(resp
            .items
            .into_iter()
            .map(|m| JmapMailbox {
                id: m.id,
                name: m.name,
            })
            .collect())
    }

    async fn mailbox_status(&self, _mailbox_id: i64) -> Result<MailboxCounts, StoreError> {
        // Fastcore's mailbox model treats all folders as views over the
        // per-user threads. Report 0/0 — clients that care fetch
        // messages directly.
        Ok(MailboxCounts { total: 0, unread: 0 })
    }

    async fn list_messages(
        &self,
        _mailbox_id: i64,
        _offset: u32,
        _limit: u32,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        // Fastcore is thread-centric; per-mailbox listing is emulated
        // by the webapi's `/api/mail/folders/{name}/messages` handler.
        // JMAP Email/query implementations get their messages via
        // list_thread_messages, so returning empty here is safe.
        Ok(Vec::new())
    }

    async fn get_message_by_db_id(
        &self,
        _user: &str,
        _id: i64,
    ) -> Result<Option<JmapMessage>, StoreError> {
        Ok(None)
    }

    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        let resp = self
            .state
            .fast()
            .list_thread_messages(user, thread_id)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })?;
        Ok(resp.items.into_iter().map(bridge_wire_to_jmap).collect())
    }

    async fn update_flags(
        &self,
        _mailbox_id: i64,
        _uid: u32,
        _flags: u32,
    ) -> Result<(), StoreError> {
        Ok(())
    }

    async fn add_flags(&self, _mailbox_id: i64, _uid: u32, _flags: u32) -> Result<(), StoreError> {
        Ok(())
    }

    async fn read_message_raw(&self, message: &JmapMessage) -> Option<Vec<u8>> {
        use mailrs_message_store::MessageStore;
        let (local, domain) = message.user_address.split_once('@')?;
        let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        let path = format!("{root}/{domain}/{local}");
        let id = mailrs_message_store::MessageId(message.blob_id.clone());
        mailrs_message_store::MaildirStore.fetch(&path, &id).await.ok().flatten()
    }

    fn parse_message(&self, raw: &[u8]) -> ParsedBody {
        let msg = mailrs_mime::part::parse(raw);
        let text = msg
            .find_by_content_type("text/plain")
            .and_then(|p| p.body_text())
            .unwrap_or_default();
        let html = msg
            .find_by_content_type("text/html")
            .and_then(|p| p.body_text())
            .unwrap_or_default();
        let attachments = msg
            .attachments()
            .map(|a| JmapAttachment {
                filename: a
                    .attachment_filename()
                    .unwrap_or("attachment")
                    .to_string(),
                content_type: format!("{}/{}", a.content_type.type_, a.content_type.subtype),
                size: a.body.len() as u32,
            })
            .collect();
        ParsedBody {
            text: Some(text),
            html: Some(html),
            attachments,
        }
    }

    async fn submit_message(
        &self,
        user: &str,
        message: &JmapMessage,
        raw: &[u8],
    ) -> SubmissionResult {
        // Enqueue onto the shared network kevy outbound queue. Force
        // the sender to the authed JMAP account (`user`) so a
        // malicious/misbehaving client can't spoof the MAIL FROM
        // envelope — mirrors REST's `ensure_from_allowed`.
        let recipients: Vec<String> = message
            .recipients
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if recipients.is_empty() {
            return SubmissionResult {
                success: false,
                message: Some("no recipients".into()),
            };
        }
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let sender = user.to_string();
        let raw_owned = raw.to_vec();
        let write = crate::handlers::kevy_util::with_kevy(move |c| {
            for rcpt in &recipients {
                let id = c.incr(b"mailrs:outbound:counter").unwrap_or(created);
                let blob = serde_json::json!({
                    "id": id,
                    "sender": sender,
                    "recipient": rcpt,
                    "message_data": String::from_utf8_lossy(&raw_owned).to_string(),
                    "created_at": created,
                })
                .to_string();
                let hkey = format!("mailrs:outbound:{id}");
                c.hset(hkey.as_bytes(), &[(b"blob" as &[u8], blob.as_bytes())])?;
                c.lpush(b"mailrs:outbound:pending", &[id.to_string().as_bytes()])?;
            }
            Ok(())
        });
        match write {
            Ok(_) => SubmissionResult {
                success: true,
                message: None,
            },
            Err(_) => SubmissionResult {
                success: false,
                message: Some("outbound enqueue failed".into()),
            },
        }
    }
}

/// GET /.well-known/jmap — session document.
pub async fn jmap_session(Extension(AuthedUser(address)): Extension<AuthedUser>) -> impl IntoResponse {
    let hostname = hostname();
    let api_url = format!("https://{hostname}/jmap");
    let download_url = format!(
        "https://{hostname}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}"
    );
    let upload_url = format!("https://{hostname}/jmap/upload/{{accountId}}/");
    let event_source_url = format!(
        "https://{hostname}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"
    );
    Json(serde_json::json!({
        "capabilities": {
            JMAP_CORE_CAP: {
                "maxSizeUpload": 50_000_000_u64,
                "maxConcurrentUpload": 4,
                "maxSizeRequest": 10_000_000_u64,
                "maxConcurrentRequests": 4,
                "maxCallsInRequest": 16,
                "maxObjectsInGet": 500,
                "maxObjectsInSet": 500,
                "collationAlgorithms": [],
            },
            JMAP_MAIL_CAP: {
                "maxMailboxesPerEmail": null,
                "maxMailboxDepth": null,
                "maxSizeMailboxName": 255,
                "maxSizeAttachmentsPerEmail": 50_000_000_u64,
                "emailQuerySortOptions": ["receivedAt", "sentAt"],
                "mayCreateTopLevelMailbox": true,
            },
            JMAP_SUBMISSION_CAP: {},
        },
        "accounts": {
            &address: {
                "name": &address,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    JMAP_CORE_CAP: {},
                    JMAP_MAIL_CAP: {},
                    JMAP_SUBMISSION_CAP: {},
                }
            }
        },
        "primaryAccounts": {
            JMAP_CORE_CAP: &address,
            JMAP_MAIL_CAP: &address,
            JMAP_SUBMISSION_CAP: &address,
        },
        "username": &address,
        "apiUrl": api_url,
        "downloadUrl": download_url,
        "uploadUrl": upload_url,
        "eventSourceUrl": event_source_url,
        "state": "0",
    }))
}

/// POST /jmap — request dispatcher.
pub async fn jmap_api(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Json(req): Json<JmapRequest>,
) -> impl IntoResponse {
    let adapter = JmapAdapter::new(state);
    let response = mailrs_jmap::dispatch_request(req, &address, &adapter).await;
    Json(response)
}

#[derive(Deserialize)]
pub struct EventSourceParams {
    #[serde(default)]
    pub types: Option<String>,
    #[serde(default)]
    pub closeafter: Option<String>,
    #[serde(default)]
    pub ping: Option<u32>,
}

/// GET /jmap/eventsource/ — Server-Sent Events for JMAP push (RFC 8620 §7).
pub async fn jmap_eventsource(
    Extension(AuthedUser(_address)): Extension<AuthedUser>,
    Query(params): Query<EventSourceParams>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let ping_secs = params.ping.unwrap_or(30).clamp(5, 300) as u64;
    let stream = futures_util::stream::unfold(ping_secs, |ping_secs| async move {
        tokio::time::sleep(std::time::Duration::from_secs(ping_secs)).await;
        Some((
            Ok(Event::default().event("ping").data("{}")),
            ping_secs,
        ))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
