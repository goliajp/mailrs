//! `/api/mail/*` REST handlers.
//!
//! Phase 3 — thin shims over `state.core_client.X()`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use mailrs_core_api::method::{mailbox as mb_wire, message as msg_wire};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// GET /api/mail/folders
pub async fn get_folders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<mb_wire::ListMailboxesResponse>, StatusCode> {
    state
        .core_client
        .list_mailboxes(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/mail/messages/{uid}
///
/// Phase 3 partial: today's REST API resolves the mailbox by name from
/// session context, but the shim takes mailbox_id via query for now.
/// Full path-compat handler will land with checklist 3.6.
#[derive(Debug, serde::Deserialize)]
pub struct UidQuery {
    pub mailbox_id: i64,
}

pub async fn get_message_raw(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<UidQuery>,
) -> Result<axum::response::Response, StatusCode> {
    let bytes = state
        .core_client
        .get_message_raw(q.mailbox_id, uid)
        .await
        .map_err(map_err)?;
    let mut resp = bytes.into_response();
    resp.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("message/rfc822"),
    );
    Ok(resp)
}

pub async fn get_message(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(uid): Path<u32>,
    axum::extract::Query(q): axum::extract::Query<UidQuery>,
) -> Result<Json<msg_wire::MessageWire>, StatusCode> {
    state
        .core_client
        .get_message_by_uid(q.mailbox_id, uid)
        .await
        .map(Json)
        .map_err(map_err)
}

/// GET /api/mail/stats
///
/// Combines unseen-count + action-count + total messages into a
/// dashboard-shaped response. webapi assembles it in-process via two
/// RPC calls so the existing frontend payload shape is preserved.
#[derive(Debug, serde::Serialize)]
pub struct MailStatsResponse {
    pub unseen: i64,
    pub action: i64,
}

/// POST /api/mail/send  — minimal v1: webapi packages a tiny RFC 5322
/// envelope and enqueues via core RPC. Frontend MUST already build the
/// final body (headers + body); webapi does not currently re-sign DKIM
/// (that happens at the sender end on actual SMTP). For the cutover this
/// matches the existing monolith behavior of treating /api/mail/send as
/// a thin enqueue wrapper.
#[derive(Debug, serde::Deserialize)]
pub struct SendRequest {
    pub to: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SendResponse {
    pub queue_id: i64,
}

pub async fn send_message(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    let message = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\n\r\n{body}",
        from = user,
        to = req.to,
        subject = req.subject,
        body = req.body,
    );
    let body_b64 = B64.encode(message.as_bytes());
    let enq = mailrs_core_api::method::outbound::EnqueueRequest {
        sender: user.clone(),
        recipient: req.to.clone(),
        original_sender: None,
        message_data_base64: body_b64,
        scheduled_at: None,
    };
    let resp = state
        .core_client
        .outbound_enqueue(&enq)
        .await
        .map_err(map_err)?;
    Ok(Json(SendResponse { queue_id: resp.id }))
}

#[derive(Debug, serde::Deserialize)]
pub struct ContactsQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_contacts_limit")]
    pub limit: u32,
}

fn default_contacts_limit() -> u32 {
    5
}

/// GET /api/contacts?q=&limit=  — sender autocomplete.
pub async fn get_contacts(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Query(q): axum::extract::Query<ContactsQuery>,
) -> Result<Json<mailrs_core_api::method::contact::SearchContactsResponse>, StatusCode> {
    state
        .core_client
        .search_contacts(&user, &q.q, q.limit)
        .await
        .map(Json)
        .map_err(map_err)
}

#[derive(Debug, serde::Deserialize)]
pub struct FeedbackRequest {
    pub sender: String,
    pub action: String,
}

/// POST /api/mail/feedback  — sender reputation feedback (block /
/// mark_vip / mark_important / etc — same vocabulary the monolith uses).
pub async fn submit_feedback(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<FeedbackRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .sender_feedback(&user, &req.sender, &req.action)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/mail/drafts
pub async fn list_drafts(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<mailrs_core_api::method::admin::DraftListResponse>, StatusCode> {
    state
        .core_client
        .list_drafts(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// POST /api/mail/drafts
pub async fn save_draft(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<mailrs_core_api::method::admin::SaveDraftRequest>,
) -> Result<Json<mailrs_core_api::method::admin::SaveDraftResponse>, StatusCode> {
    state
        .core_client
        .save_draft(&user, &req)
        .await
        .map(Json)
        .map_err(map_err)
}

/// DELETE /api/mail/drafts/{id}
pub async fn delete_draft(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .delete_draft(&user, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/mail/signatures
pub async fn list_signatures(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<mailrs_core_api::method::admin::SignatureListResponse>, StatusCode> {
    state
        .core_client
        .list_signatures(&user)
        .await
        .map(Json)
        .map_err(map_err)
}

/// POST /api/mail/signatures
pub async fn save_signature(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Json(req): Json<mailrs_core_api::method::admin::SaveSignatureRequest>,
) -> Result<Json<mailrs_core_api::method::admin::SaveSignatureResponse>, StatusCode> {
    state
        .core_client
        .save_signature(&user, &req)
        .await
        .map(Json)
        .map_err(map_err)
}

/// DELETE /api/mail/signatures/{id}
pub async fn delete_signature(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .core_client
        .delete_signature(&user, id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(map_err)
}

/// GET /api/queue  — outbound queue depths for ops dashboards.
pub async fn get_queue_stats(
    State(state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
) -> Result<Json<mailrs_core_api::method::outbound::QueueStatsResponse>, StatusCode> {
    state
        .core_client
        .outbound_stats()
        .await
        .map(Json)
        .map_err(map_err)
}

pub async fn get_mail_stats(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<MailStatsResponse>, StatusCode> {
    let unseen = state
        .core_client
        .unseen_count(&user)
        .await
        .map_err(map_err)?;
    let action = state
        .core_client
        .action_count(&user)
        .await
        .map_err(map_err)?;
    Ok(Json(MailStatsResponse {
        unseen: unseen.count,
        action: action.count,
    }))
}
