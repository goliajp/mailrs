//! JMAP session document + `POST /jmap` dispatcher + SSE event stream.
//!
//! Ports the monolith at `crates/server/src/web/jmap.rs`. The
//! `MailStore` implementation delegates to fastcore RPCs instead of
//! `PgMailboxStore`. Data flow is unchanged for JMAP clients (RFC 8620
//! / RFC 8621).

use std::sync::Arc;

use async_trait::async_trait;
use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
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

    /// Best-effort mailbox → (wire, owner) lookup. JMAP passes just
    /// a mailbox_id which is a per-user synthetic in fastcore; we
    /// discover which user by scanning every registered account until
    /// we find one whose list_mailboxes returns a match. Cheap because
    /// list_mailboxes is O(1) per account (INBOX/Sent/Drafts/Junk/Trash).
    async fn resolve_mailbox(
        &self,
        mailbox_id: i64,
    ) -> Result<(mailrs_core_api::method::mailbox::MailboxWire, String), StoreError> {
        let accounts = self
            .state
            .core
            .list_accounts()
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })?;
        for a in accounts.items {
            let addr = a.address;
            if let Ok(list) = self.state.core.list_mailboxes(&addr).await
                && let Some(m) = list.items.into_iter().find(|m| m.id == mailbox_id)
            {
                return Ok((m, addr));
            }
        }
        Err(Box::new(std::io::Error::other("mailbox not found")))
    }
}

fn bridge_wire_to_jmap(w: mailrs_core_api::method::message::MessageWire) -> JmapMessage {
    // MessageWire.id is a PG-era holdover, always 0 under the kevy-only
    // fastcore architecture (see 2026-07-08 timeline-duplication bug).
    // Synthesise a stable per-account JMAP identity from (mailbox_id,
    // uid) — the pair the IMAP layer already uses to key flag updates.
    // Rendered on the JMAP wire as `msg-{synthetic_id}`.
    let synthetic_id: i64 = ((w.mailbox_id) << 32) | i64::from(w.uid);
    JmapMessage {
        id: synthetic_id,
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
            .core
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

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxCounts, StoreError> {
        // Fastcore's mailboxes are synthetic (INBOX = 1, Sent = 2, …).
        // Total = uidnext − 1 from list_mailboxes; unread = the
        // user-scoped unseen_count for INBOX, 0 elsewhere.
        // Prior version reported 0/0 always — iPhone Mail then showed
        // every folder empty even when they weren't.
        let (mb, user) = self.resolve_mailbox(mailbox_id).await?;
        let total = mb.uidnext.saturating_sub(1);
        let unread = if mb.name.eq_ignore_ascii_case("INBOX") {
            self.state
                .core
                .unseen_count(&user)
                .await
                .map(|r| r.count.max(0) as u32)
                .unwrap_or(0)
        } else {
            0
        };
        Ok(MailboxCounts { total, unread })
    }

    async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        // Walk the user's conversations for the resolved mailbox's
        // folder, then flatten each thread's messages. Sufficient for
        // Email/query with basic sort — full CONDSTORE / cursor
        // paging tracks in a follow-up.
        let (mb, user) = self.resolve_mailbox(mailbox_id).await?;
        let mut out = Vec::new();
        let req = mailrs_core_api::method::conversation::ListConversationsRequest {
            filter: mailrs_core_api::types::ConversationFilter {
                limit: (offset + limit).min(500),
                folder: Some(mb.name.clone()),
                ..Default::default()
            },
        };
        let resp = self
            .state
            .core
            .list_conversations(&user, &req)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })?;
        for conv in resp.items.into_iter().skip(offset as usize) {
            if out.len() as u32 >= limit {
                break;
            }
            let msgs = self
                .state
                .core
                .list_thread_messages(&user, &conv.thread_id)
                .await
                .map(|r| r.items)
                .unwrap_or_default();
            for w in msgs {
                out.push(bridge_wire_to_jmap(w));
                if out.len() as u32 >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    async fn get_message_by_db_id(
        &self,
        _user: &str,
        _id: i64,
    ) -> Result<Option<JmapMessage>, StoreError> {
        // The webapi doesn't own a global message-id → wire index —
        // JMAP handlers that need this take the mailbox_id + uid
        // route instead. Returning None matches JMAP Email/get with
        // an unknown blobId.
        Ok(None)
    }

    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<JmapMessage>, StoreError> {
        let resp = self
            .state
            .core
            .list_thread_messages(user, thread_id)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })?;
        Ok(resp.items.into_iter().map(bridge_wire_to_jmap).collect())
    }

    async fn update_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<(), StoreError> {
        let (_mb, user) = self.resolve_mailbox(mailbox_id).await?;
        let req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags };
        self.state
            .core
            .set_message_flags(&user, uid, &req)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })
    }

    async fn add_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<(), StoreError> {
        // Read-modify-write: fetch current flags, OR the new bits in,
        // write back. Ties into the same fastcore RPC as update_flags.
        let (_mb, user) = self.resolve_mailbox(mailbox_id).await?;
        let cur = self
            .state
            .core
            .get_message_by_uid_for_user(&user, uid)
            .await
            .map(|w| w.flags)
            .unwrap_or(0);
        let req = mailrs_core_api::method::admin::SetMessageFlagsRequest { flags: cur | flags };
        self.state
            .core
            .set_message_flags(&user, uid, &req)
            .await
            .map_err(|e| -> StoreError { Box::new(std::io::Error::other(e.to_string())) })
    }

    async fn read_message_raw(&self, message: &JmapMessage) -> Option<Vec<u8>> {
        use mailrs_message_store::MessageStore;
        let (local, domain) = message.user_address.split_once('@')?;
        let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
        let path = format!("{root}/{domain}/{local}");
        let id = mailrs_message_store::MessageId(message.blob_id.clone());
        mailrs_message_store::MaildirStore
            .fetch(&path, &id)
            .await
            .ok()
            .flatten()
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
                filename: a.attachment_filename().unwrap_or("attachment").to_string(),
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
                use base64::Engine as _;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_owned);
                let blob = serde_json::json!({
                    "id": id,
                    "sender": sender,
                    "recipient": rcpt,
                    "message_data_b64": b64,
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
pub async fn jmap_session(
    Extension(AuthedUser(address)): Extension<AuthedUser>,
) -> impl IntoResponse {
    let hostname = hostname();
    let api_url = format!("https://{hostname}/jmap");
    let download_url =
        format!("https://{hostname}/jmap/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}");
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
        Some((Ok(Event::default().event("ping").data("{}")), ping_secs))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
