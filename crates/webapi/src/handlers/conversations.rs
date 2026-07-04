//! `/api/conversations*` REST handlers — thin shims that delegate to the
//! core RPC client.
//!
//! Phase 3.5 — replaces the monolith's direct `state.mailbox_store.X()`
//! calls (REST agent inventory in `docs/CURRENT_STATE_FROZEN.md` §0.2)
//! with `state.core_client.X()` RPC calls.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use mailrs_core_api::method::conversation as wire;
use mailrs_core_api::types::ConversationFilter;
use serde::Deserialize;

use crate::WebState;

/// Resolved user identity carried via axum Extension by the auth layer
/// (set by `session::session_auth_middleware`).
#[derive(Debug, Clone)]
pub struct AuthedUser(pub String);

/// Optional display name from the session blob — set by the auth layer
/// when available, blank when the dev fallback header path is used.
#[derive(Debug, Clone, Default)]
pub struct AuthedDisplayName(pub String);

/// GET /api/conversations  — query-string filter, returns the list.
#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default, alias = "before")]
    pub before_ts: Option<i64>,
    pub category: Option<String>,
    pub folder: Option<String>,
    #[serde(default)]
    pub archived: bool,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub section: Option<String>,
}

fn default_limit() -> u32 {
    50
}

/// Wire shape the React UI expects for /api/conversations.
///
/// Same as monolith's `ConversationResponse` — critical difference from
/// fastcore's `ConversationSummaryWire` is `participants` is a `Vec<String>`
/// (split by comma) instead of the raw csv string. UI does
/// `convo.participants[0]` which on a plain string returns the first
/// CHARACTER, not the first sender.
#[derive(serde::Serialize)]
pub struct ConversationResponse {
    pub thread_id: String,
    pub subject: String,
    pub participants: Vec<String>,
    pub message_count: u32,
    pub unread_count: u32,
    pub last_date: i64,
    pub category: String,
    pub flagged: bool,
    pub snippet: String,
    pub pinned: bool,
    pub archived: bool,
    pub importance_level: String,
    pub importance_score: f32,
    pub requires_action: bool,
    pub last_sender: String,
    pub received_count: u32,
    pub sent_count: u32,
}

impl From<mailrs_core_api::types::ConversationSummaryWire> for ConversationResponse {
    fn from(w: mailrs_core_api::types::ConversationSummaryWire) -> Self {
        let participants: Vec<String> = w
            .participants
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let received_count = w.message_count.saturating_sub(w.sent_count);
        Self {
            thread_id: w.thread_id,
            subject: w.subject,
            participants,
            message_count: w.message_count,
            unread_count: w.unread_count,
            last_date: w.last_date,
            category: w.category,
            flagged: w.flagged,
            snippet: w.snippet,
            pinned: w.pinned,
            archived: w.archived,
            importance_level: w.importance_level,
            importance_score: w.importance_score,
            requires_action: w.requires_action,
            last_sender: w.last_sender,
            received_count,
            sent_count: w.sent_count,
        }
    }
}

pub async fn get_conversations(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ConversationResponse>>, StatusCode> {
    let req = wire::ListConversationsRequest {
        filter: ConversationFilter {
            limit: q.limit,
            before_ts: q.before_ts,
            category: q.category,
            domains: None,
            archived: q.archived,
            folder: q.folder,
            unread: q.unread,
            starred: q.starred,
            section: q.section,
        },
    };
    let resp = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(map_err)?;
    Ok(Json(resp.items.into_iter().map(Into::into).collect()))
}

/// GET /api/conversations/categories — return bare Vec<CategoryCount>
/// (monolith shape, not wrapped in `{"categories": [...]}`).
pub async fn get_categories(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<wire::CategoryCount>>, StatusCode> {
    state
        .core
        .conversation_categories(&user)
        .await
        .map(|r| Json(r.categories))
        .map_err(map_err)
}

/// GET /api/conversations/action-count — return bare `{count: N}`
/// (already the response shape, but as flat i64 not the response struct).
pub async fn get_action_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .core
        .action_count(&user)
        .await
        .map(|r| Json(serde_json::json!({ "count": r.count })))
        .map_err(map_err)
}

/// Wire shape UI expects for a message. Mirrors monolith's
/// `ThreadMessageResponse` — critical fields the UI reaches into
/// unconditionally (`attachments.length`, `text_body`, `html_body`,
/// `people`, `dates`, ...). Return default empty arrays / null when
/// the source `MessageWire` lacks the analysis fields.
#[derive(serde::Serialize)]
pub struct ThreadMessageResponse {
    pub id: i64,
    pub uid: u32,
    pub sender: String,
    pub recipients: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<String>,
    pub subject: String,
    pub flags: u32,
    pub internal_date: i64,
    pub message_id: String,
    pub text_body: Option<String>,
    pub html_body: Option<String>,
    pub attachments: Vec<serde_json::Value>,
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
    pub new_content: Option<String>,
    pub importance_level: String,
    pub importance_score: f32,
    pub is_bulk_sender: bool,
    pub has_tracking_pixel: bool,
    pub requires_action: bool,
    pub sender_intent: String,
    pub action_deadline: Option<String>,
}

impl ThreadMessageResponse {
    fn from_wire_no_body(w: mailrs_core_api::method::message::MessageWire) -> Self {
        Self {
            id: w.id,
            uid: w.uid,
            sender: w.sender,
            recipients: w.recipients,
            cc: None,
            subject: w.subject,
            flags: w.flags,
            internal_date: w.internal_date,
            message_id: w.message_id,
            text_body: None,
            html_body: None,
            attachments: Vec::new(),
            category: String::from("inbox"),
            risk_score: 0,
            risk_reason: String::new(),
            summary: String::new(),
            people: serde_json::json!([]),
            dates: serde_json::json!([]),
            amounts: serde_json::json!([]),
            action_items: serde_json::json!([]),
            ai_analyzed: false,
            clean_text: None,
            new_content: None,
            importance_level: String::from("normal"),
            importance_score: 0.0,
            is_bulk_sender: false,
            has_tracking_pixel: false,
            requires_action: false,
            sender_intent: String::new(),
            action_deadline: None,
        }
    }
}

/// Extract the `Cc:` header from the raw RFC 5322 source. Follows folded
/// continuation lines (leading whitespace = continuation of prior header).
/// Returns `None` when no Cc header is present or when it's empty.
fn extract_cc_header(data: &[u8]) -> Option<String> {
    // Header block ends at the first blank line (CRLF CRLF or LF LF).
    let head_end = memchr_bytes(data, b"\r\n\r\n")
        .or_else(|| memchr_bytes(data, b"\n\n"))
        .unwrap_or(data.len());
    let head = &data[..head_end];
    let mut lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    for l in lines.iter_mut() {
        if l.last() == Some(&b'\r') {
            *l = &l[..l.len() - 1];
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let ll = lines[i];
        if !ll.len().ge(&3) {
            i += 1;
            continue;
        }
        if ll.get(..3).is_some_and(|s| s.eq_ignore_ascii_case(b"cc:")) {
            let mut val: Vec<u8> = ll[3..].trim_ascii_start().to_vec();
            let mut j = i + 1;
            while j < lines.len() {
                let cont = lines[j];
                if cont.first().is_some_and(|c| *c == b' ' || *c == b'\t') {
                    val.push(b' ');
                    val.extend_from_slice(cont.trim_ascii());
                    j += 1;
                } else {
                    break;
                }
            }
            let s = String::from_utf8_lossy(&val).trim().to_string();
            return if s.is_empty() { None } else { Some(s) };
        }
        i += 1;
    }
    None
}

/// Extract the `Date:` header epoch (UTC seconds) from raw RFC 5322
/// bytes. Used to override `wire.internal_date` at read-time when the
/// stored value looks stale — the fastcore self-heal used to parse
/// dates without a proper RFC 2822 parser (dropped timezones, missing
/// dates fell to 0 → 1970), so historic threads still carry those
/// bad epochs. Re-parsing at read time fixes the sort order in the UI
/// without a bulk re-heal pass. Same folding rules as `extract_cc_header`.
fn extract_date_header_epoch(data: &[u8]) -> Option<i64> {
    let head_end = memchr_bytes(data, b"\r\n\r\n")
        .or_else(|| memchr_bytes(data, b"\n\n"))
        .unwrap_or(data.len());
    let head = &data[..head_end];
    let mut lines: Vec<&[u8]> = head.split(|&b| b == b'\n').collect();
    for l in lines.iter_mut() {
        if l.last() == Some(&b'\r') {
            *l = &l[..l.len() - 1];
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let ll = lines[i];
        if ll
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case(b"date:"))
        {
            let mut val: Vec<u8> = ll[5..].trim_ascii_start().to_vec();
            let mut j = i + 1;
            while j < lines.len() {
                let cont = lines[j];
                if cont.first().is_some_and(|c| *c == b' ' || *c == b'\t') {
                    val.push(b' ');
                    val.extend_from_slice(cont.trim_ascii());
                    j += 1;
                } else {
                    break;
                }
            }
            let s = String::from_utf8_lossy(&val).trim().to_string();
            return parse_rfc2822_epoch(&s);
        }
        i += 1;
    }
    None
}

/// Same retry ladder as fastcore's `parse_rfc5322_date`. Local copy
/// keeps webapi self-contained (chrono is already a transitive dep).
fn parse_rfc2822_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt.timestamp());
    }
    let no_comment = s.split(" (").next().unwrap_or(s).trim_end();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt.timestamp());
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow)
        .ok()
        .map(|dt| dt.timestamp())
}

fn memchr_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Parse `data` (raw RFC-5322 bytes) into text/html body + attachments.
/// Same pipeline as monolith's `crate::message_util::parse_message`.
fn parse_body(data: &[u8]) -> (Option<String>, Option<String>, Vec<serde_json::Value>) {
    let root = mailrs_mime::parse(data);
    let mut text_body: Option<String> = None;
    let mut html_body: Option<String> = None;
    for part in root.walk() {
        let mt = part.content_type.mime_type();
        if text_body.is_none() && mt == "text/plain" {
            text_body = part.body_text();
        } else if html_body.is_none() && mt == "text/html" {
            html_body = part.body_text();
        }
        if text_body.is_some() && html_body.is_some() {
            break;
        }
    }
    if text_body.is_none() && html_body.is_none() && root.children.is_empty() {
        if root.content_type.type_ == "text" {
            text_body = root.body_text();
        } else {
            text_body = Some(String::from_utf8_lossy(data).into_owned());
        }
    }
    let text_body = text_body.or_else(|| {
        html_body
            .as_deref()
            .and_then(|html| html2text::from_read(html.as_bytes(), 80).ok())
    });
    let attachments: Vec<serde_json::Value> = root
        .attachments()
        .map(|att| {
            let filename = att.attachment_filename().unwrap_or("unnamed").to_string();
            let mt = att.content_type.mime_type();
            let mt = if mt.ends_with('/') || mt.starts_with('/') {
                "application/octet-stream".to_string()
            } else {
                mt
            };
            serde_json::json!({
                "filename": filename,
                "content_type": mt,
                "size": att.body.len() as u32,
            })
        })
        .collect();
    (text_body, html_body, attachments)
}

/// Public alias for the MCP handler to reuse the same body-enrichment
/// pipeline REST uses. Same signature; keeps `enrich_with_body`
/// module-private while giving MCP a stable seam.
pub async fn enrich_with_body_public(
    store: &dyn mailrs_message_store::MessageStore,
    maildir_root: &str,
    user: &str,
    w: mailrs_core_api::method::message::MessageWire,
) -> ThreadMessageResponse {
    enrich_with_body(store, maildir_root, user, w).await
}

/// Enrich a MessageWire with body content read from maildir.
///
/// Handles Maildir++ subfolders: fastcore's self-heal stores blob_ref
/// as `<subfolder>/<filename>` for files under `.Sent/`, `.Drafts/`,
/// etc. INBOX files stay as bare `<filename>`. We split on the first
/// `/` and append the subfolder segment to the maildir path so
/// `MaildirStore::fetch` opens the right sub-maildir.
async fn enrich_with_body(
    store: &dyn mailrs_message_store::MessageStore,
    maildir_root: &str,
    user: &str,
    w: mailrs_core_api::method::message::MessageWire,
) -> ThreadMessageResponse {
    let mut r = ThreadMessageResponse::from_wire_no_body(w.clone());
    if let Some((path, id)) =
        crate::handlers::messages::blob_ref_location(maildir_root, user, &w.blob_ref)
        && let Ok(Some(bytes)) = store.fetch(&path, &id).await
    {
        let (t, h, a) = parse_body(&bytes);
        r.text_body = t;
        r.html_body = h;
        r.attachments = a;
        r.cc = extract_cc_header(&bytes);
        // Repair a stale internal_date at read time. Historic
        // messages had wire.internal_date = 0 (1970) whenever the
        // old fastcore date parser choked on the header — replace
        // with the freshly parsed epoch so the timeline sorts
        // right without needing a bulk re-heal. Only overrides
        // when we get a positive parse and the stored value is
        // clearly stale (<= 0 or older than the header epoch).
        if let Some(hdr_epoch) = extract_date_header_epoch(&bytes)
            && (r.internal_date <= 0 || r.internal_date > hdr_epoch + 86_400)
        {
            r.internal_date = hdr_epoch;
        }
    }
    r
}

/// GET /api/conversations/{thread_id} — return Vec<ThreadMessageResponse>
/// with monolith's exact wire shape (attachments, text_body, ...) so the
/// React UI can safely reach into arrays without null-guards.
pub async fn get_thread_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<Vec<ThreadMessageResponse>>, StatusCode> {
    let resp = state
        .core
        .list_thread_messages(&user, &thread_id)
        .await
        .map_err(map_err)?;
    let maildir_root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let store = mailrs_message_store::MaildirStore;
    let mut items = Vec::with_capacity(resp.items.len());
    for w in resp.items {
        items.push(enrich_with_body(&store, &maildir_root, &user, w).await);
    }
    Ok(Json(items))
}

/// Batch mutation request/response — matches the UI's `useBatchMutation`.
#[derive(Debug, serde::Deserialize)]
pub struct BatchRequest {
    pub action: String,
    pub thread_ids: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BatchResponse {
    pub failed: u32,
    pub message: Option<String>,
    pub processed: u32,
    pub success: bool,
}

/// POST /api/conversations/batch — apply the same mutation across many
/// threads. Fires each individually against fastcore (kevy mutations are
/// idempotent + fast, ~2 ms each). Runs sequentially; a partial failure
/// still lets the successes stick.
pub async fn batch_mutation(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, StatusCode> {
    let action = req.action.as_str();
    let mut processed = 0u32;
    let mut failed = 0u32;
    for tid in &req.thread_ids {
        let f = &state.core;
        let r = match action {
            "read" => f.mark_thread_read(&user, tid).await.map(|_| ()),
            "unread" => f.mark_thread_unread(&user, tid).await.map(|_| ()),
            "star" => f.star_thread(&user, tid).await.map(|_| ()),
            "unstar" => f.unstar_thread(&user, tid).await.map(|_| ()),
            "archive" => f.archive_thread(&user, tid).await.map(|_| ()),
            "unarchive" => f.unarchive_thread(&user, tid).await.map(|_| ()),
            "delete" => f.delete_thread(&user, tid).await.map(|_| ()),
            _ => Err(mailrs_core_api::error::CoreApiError::Internal(format!(
                "unknown batch action: {action}"
            ))),
        };
        match r {
            Ok(_) => processed += 1,
            Err(_) => failed += 1,
        }
    }
    Ok(Json(BatchResponse {
        failed,
        message: None,
        processed,
        success: failed == 0,
    }))
}

/// POST /api/conversations/{thread_id}/read
pub async fn mark_thread_read(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_thread_read(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/mark-all-read — sweep every unread thread
/// for the current user. The old "Mark all as read" button was only
/// batching the currently-loaded pagination slice; with 99+ unread
/// spread across pages the tail stayed untouched. This endpoint fixes
/// that by walking the has_unread zset server-side.
pub async fn mark_all_read(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let flipped = state
        .core
        .mark_all_conversations_read(&user)
        .await
        .map_err(map_err)?;
    Ok(Json(
        serde_json::json!({ "success": true, "flipped": flipped }),
    ))
}

/// POST /api/conversations/{thread_id}/star
pub async fn star_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .star_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/archive
pub async fn archive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .archive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unread
pub async fn mark_thread_unread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .mark_thread_unread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unstar
pub async fn unstar_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unstar_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/pin
pub async fn pin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .pin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unpin
pub async fn unpin_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unpin_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/unarchive
pub async fn unarchive_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unarchive_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// POST /api/conversations/{thread_id}/dismiss-action
pub async fn dismiss_action(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .dismiss_thread_action(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}
pub async fn delete_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .delete_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct SnoozeBody {
    pub snoozed_until: i64,
}

/// PUT /api/conversations/{thread_id}/snooze
pub async fn snooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
    Json(req): Json<SnoozeBody>,
) -> Result<StatusCode, StatusCode> {
    let wire_req = mailrs_core_api::method::thread::SnoozeRequest {
        snoozed_until: req.snoozed_until,
    };
    state
        .core
        .snooze_thread(&user, &thread_id, &wire_req)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// DELETE /api/conversations/{thread_id}/snooze
pub async fn unsnooze_thread(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .core
        .unsnooze_thread(&user, &thread_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/conversations/unseen-count — returns `{"count": N}` inline
/// (monolith shape).
pub async fn get_unseen_count(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .core
        .unseen_count(&user)
        .await
        .map(|r| Json(serde_json::json!({ "count": r.count })))
        .map_err(map_err)
}

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    let code = e.status_code();
    StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod cc_tests {
    use super::extract_cc_header;

    #[test]
    fn returns_none_when_no_cc_header() {
        let m = b"From: a@x\r\nTo: b@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }

    #[test]
    fn extracts_single_line_cc() {
        let m = b"From: a@x\r\nTo: b@x\r\nCc: c@x, d@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x, d@x"));
    }

    #[test]
    fn extracts_folded_cc() {
        let m = b"From: a@x\r\nCc: c@x,\r\n d@x,\r\n\te@x\r\nSubject: hi\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x, d@x, e@x"));
    }

    #[test]
    fn case_insensitive_header_name() {
        let m = b"From: a@x\r\nCC: c@x\r\n\r\nbody";
        assert_eq!(extract_cc_header(m).as_deref(), Some("c@x"));
    }

    #[test]
    fn stops_at_header_terminator() {
        let m = b"From: a@x\r\n\r\nCc: fake@x\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }

    #[test]
    fn empty_cc_returns_none() {
        let m = b"From: a@x\r\nCc:   \r\n\r\nbody";
        assert_eq!(extract_cc_header(m), None);
    }
}
