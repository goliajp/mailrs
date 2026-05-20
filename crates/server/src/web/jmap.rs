//! Axum wiring for mailrs-jmap.
//!
//! `mailrs_jmap` ships the dispatcher + per-method handlers framework-free.
//! This module:
//!   1. Implements `mailrs_jmap::MailStore` for a server-side adapter that
//!      wraps `mailrs_mailbox::MailboxStore` + bridges types.
//!   2. Exposes the `GET /.well-known/jmap` session document and the
//!      `POST /jmap` request endpoint as axum handlers.
//!   3. Keeps the JMAP push (SSE) endpoint here, since the event-source
//!      stream is tied to our broadcast-channel runtime.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use mailrs_jmap::dispatch::{
    JmapRequest, JMAP_CORE_CAP, JMAP_MAIL_CAP, JMAP_SUBMISSION_CAP,
};
use mailrs_jmap::store::{MailStore, StoreError};
use mailrs_jmap::types::{
    Attachment as JmapAttachment, Mailbox as JmapMailbox, MailboxCounts, Message as JmapMessage,
    ParsedBody, SubmissionResult,
};

use super::{AuthUser, WebState};

/// Bridge between `mailrs_mailbox::MailboxStore` and `mailrs_jmap::MailStore`.
///
/// Held by every handler call; cheap to clone because `WebState` is `Arc`'d.
pub(super) struct JmapAdapter {
    state: Arc<WebState>,
}

impl JmapAdapter {
    pub fn new(state: Arc<WebState>) -> Self {
        Self { state }
    }

    fn mailbox_store(&self) -> Result<&Arc<mailrs_mailbox::MailboxStore>, StoreError> {
        self.state.mailbox_store.as_ref().ok_or_else(|| -> StoreError {
            "mailbox store not available".into()
        })
    }
}

fn bridge_mailbox(mb: mailrs_mailbox::types::Mailbox) -> JmapMailbox {
    JmapMailbox {
        id: mb.id,
        name: mb.name,
    }
}

fn bridge_message(m: mailrs_mailbox::types::MessageMeta) -> JmapMessage {
    JmapMessage {
        id: m.id,
        mailbox_id: m.mailbox_id,
        uid: m.uid,
        sender: m.sender,
        recipients: m.recipients,
        subject: m.subject,
        date: m.date,
        size: m.size,
        flags: m.flags,
        internal_date: m.internal_date,
        message_id: m.message_id,
        in_reply_to: m.in_reply_to,
        thread_id: m.thread_id,
        user_address: m.user_address,
        new_content: m.new_content,
        // mailrs uses the maildir id as the on-disk blob identifier.
        blob_id: m.maildir_id,
    }
}

#[async_trait]
impl MailStore for JmapAdapter {
    async fn list_mailboxes(&self, user: &str) -> Result<Vec<JmapMailbox>, StoreError> {
        let store = self.mailbox_store()?;
        let rows = store
            .list_mailboxes(user)
            .await
            .map_err(|e| Box::new(e) as StoreError)?;
        Ok(rows.into_iter().map(bridge_mailbox).collect())
    }

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxCounts, StoreError> {
        let store = self.mailbox_store()?;
        let (total, unread) = store
            .mailbox_status(mailbox_id)
            .await
            .map_err(|e| Box::new(e) as StoreError)?;
        Ok(MailboxCounts { total, unread })
    }

    async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        let store = self.mailbox_store()?;
        let rows = store
            .list_messages(mailbox_id, offset, limit)
            .await
            .map_err(|e| Box::new(e) as StoreError)?;
        Ok(rows.into_iter().map(bridge_message).collect())
    }

    async fn get_message_by_db_id(
        &self,
        user: &str,
        id: i64,
    ) -> Result<Option<JmapMessage>, StoreError> {
        let store = self.mailbox_store()?;
        let row = store
            .get_message_by_db_id(user, id)
            .await
            .map_err(|e| Box::new(e) as StoreError)?;
        Ok(row.map(bridge_message))
    }

    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        let store = self.mailbox_store()?;
        let rows = store
            .list_thread_messages(user, thread_id, None)
            .await
            .map_err(|e| Box::new(e) as StoreError)?;
        Ok(rows.into_iter().map(bridge_message).collect())
    }

    async fn update_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<(), StoreError> {
        let store = self.mailbox_store()?;
        store
            .update_flags(mailbox_id, uid, flags)
            .await
            .map(|_| ())
            .map_err(|e| Box::new(e) as StoreError)
    }

    async fn add_flags(
        &self,
        mailbox_id: i64,
        uid: u32,
        flags: u32,
    ) -> Result<(), StoreError> {
        let store = self.mailbox_store()?;
        store
            .add_flags(mailbox_id, uid, flags)
            .await
            .map(|_| ())
            .map_err(|e| Box::new(e) as StoreError)
    }

    async fn read_message_raw(&self, message: &JmapMessage) -> Option<Vec<u8>> {
        crate::message_util::read_message_raw(
            &self.state.maildir_root,
            &message.user_address,
            &message.blob_id,
        )
    }

    fn parse_message(&self, raw: &[u8]) -> ParsedBody {
        let (text, html, attachments) = crate::message_util::parse_message(raw);
        ParsedBody {
            text,
            html,
            attachments: attachments
                .into_iter()
                .map(|a| JmapAttachment {
                    filename: a.filename,
                    content_type: a.content_type,
                    size: a.size,
                })
                .collect(),
        }
    }

    async fn submit_message(
        &self,
        user: &str,
        message: &JmapMessage,
        raw: &[u8],
    ) -> SubmissionResult {
        let to: Vec<String> = message
            .recipients
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let Json(result) = crate::web::mail::deliver_message(
            &self.state,
            user,
            &to,
            &[],
            &[],
            raw,
            &message.message_id,
            message.date,
        )
        .await;

        SubmissionResult {
            success: result.success,
            message: result.message,
        }
    }
}

pub(super) async fn jmap_session(
    AuthUser { address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let api_url = format!("https://{}/jmap", state.hostname);
    let download_url = format!(
        "https://{}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}",
        state.hostname
    );
    let upload_url = format!("https://{}/jmap/upload/{{accountId}}/", state.hostname);
    let event_source_url = format!(
        "https://{}/jmap/eventsource/?types={{types}}&closeafter={{closeafter}}&ping={{ping}}",
        state.hostname
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
                "collationAlgorithms": []
            },
            JMAP_MAIL_CAP: {
                "maxMailboxesPerEmail": null,
                "maxMailboxDepth": null,
                "maxSizeMailboxName": 255,
                "maxSizeAttachmentsPerEmail": 50_000_000_u64,
                "emailQuerySortOptions": ["receivedAt", "sentAt"],
                "mayCreateTopLevelMailbox": true
            },
            JMAP_SUBMISSION_CAP: {}
        },
        "accounts": {
            &address: {
                "name": &address,
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    JMAP_CORE_CAP: {},
                    JMAP_MAIL_CAP: {},
                    JMAP_SUBMISSION_CAP: {}
                }
            }
        },
        "primaryAccounts": {
            JMAP_CORE_CAP: &address,
            JMAP_MAIL_CAP: &address,
            JMAP_SUBMISSION_CAP: &address
        },
        "username": &address,
        "apiUrl": api_url,
        "downloadUrl": download_url,
        "uploadUrl": upload_url,
        "eventSourceUrl": event_source_url,
        "state": "0"
    }))
}

pub(super) async fn jmap_api(
    auth_user: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(request): Json<JmapRequest>,
) -> impl IntoResponse {
    let adapter = JmapAdapter::new(state);
    let response = mailrs_jmap::dispatch_request(request, &auth_user.address, &adapter).await;
    Json(response)
}

// JMAP EventSource (SSE push notifications).
//
// Kept here because the broadcast-channel + auth context are server-side
// concerns; the wire format is RFC 8620 §7 but the runtime is ours.
pub(super) async fn jmap_eventsource(
    AuthUser { address, .. }: AuthUser,
    Query(params): Query<EventSourceParams>,
    State(state): State<Arc<WebState>>,
) -> axum::response::sse::Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::{Event, KeepAlive, Sse};

    let ping_secs = params.ping.unwrap_or(30).clamp(5, 300) as u64;
    let rx = state.event_bus.subscribe();

    let stream = futures_util::stream::unfold(
        (rx, address, ping_secs),
        |(mut rx, address, ping_secs)| async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(ping_secs)) => {
                        let event = Event::default().event("ping").data("{}");
                        return Some((Ok(event), (rx, address, ping_secs)));
                    }
                    result = rx.recv() => {
                        match result {
                            Ok(ev) => {
                                if let Ok(json) = serde_json::to_string(&ev)
                                    && json.contains(&address) {
                                        let data = serde_json::json!({
                                            "@type": "StateChange",
                                            "changed": {
                                                &address: {
                                                    "Email": chrono::Utc::now().timestamp().to_string()
                                                }
                                            }
                                        });
                                        let event = Event::default().event("state").data(data.to_string());
                                        return Some((Ok(event), (rx, address, ping_secs)));
                                    }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => return None,
                        }
                    }
                }
            }
        },
    );

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}

#[derive(Deserialize)]
pub(super) struct EventSourceParams {
    #[serde(default)]
    #[allow(dead_code)]
    pub types: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub closeafter: Option<String>,
    #[serde(default)]
    pub ping: Option<u32>,
}
