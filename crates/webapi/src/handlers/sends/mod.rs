//! The Send list — one row per send, with delivery status
//! (RFC 20260730-send-status S3).
//!
//! Distinct from the Sent conversation axis this will replace. That axis
//! lists conversations, and status is a property of an attempt: three
//! sends in one thread can be delivered, failed and retrying at once, and
//! a conversation row has nowhere to put that, nor anywhere to hang
//! "re-edit this one" when only one of the three failed.
//!
//! Nothing in the UI reads this yet. `:shadow` answers whether it is safe
//! to.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

use mailrs_core_sidestate::families::send::Status;
use mailrs_core_sidestate::families::send_read;

mod redraft;
mod resend;

pub use redraft::*;
pub use resend::*;

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::connect(&url).map_err(std::io::Error::other)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, serde::Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    /// One of the status names. An unrecognised value is rejected rather
    /// than quietly widened to "everything" — a filter that silently
    /// stops filtering shows the user more than they asked for and looks
    /// like it worked.
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    50
}

#[derive(Debug, serde::Serialize)]
pub struct RecipientResponse {
    pub recipient: String,
    pub delivered: bool,
    pub pending: bool,
    /// The remote's reply code, or 0 before any server answered.
    pub code: u16,
    /// The remote's text, verbatim.
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SendResponse {
    pub send_id: String,
    pub thread_id: String,
    pub subject: String,
    pub to: Vec<String>,
    pub created_at: i64,
    pub status: String,
    /// Whether resend / re-edit have envelope bytes to work from. The UI
    /// needs this to decide whether to offer the buttons at all, rather
    /// than offering them and doing nothing.
    pub can_resend: bool,
    pub resent_from: Option<String>,
    pub recipients: Vec<RecipientResponse>,
}

impl From<send_read::SendListItem> for SendResponse {
    fn from(i: send_read::SendListItem) -> Self {
        let can_resend = i.can_resend();
        Self {
            send_id: i.send_id,
            thread_id: i.thread_id,
            subject: i.subject,
            to: i
                .to_csv
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            created_at: i.created_at,
            status: i.status.as_str().to_string(),
            can_resend,
            resent_from: i.resent_from,
            recipients: i
                .recipients
                .into_iter()
                .map(|r| RecipientResponse {
                    recipient: r.recipient,
                    delivered: r.delivered,
                    pending: r.pending,
                    code: r.code,
                    message: r.message,
                })
                .collect(),
        }
    }
}

/// `GET /api/mail/sends?limit=&offset=&status=`
pub async fn list_sends(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SendResponse>>, StatusCode> {
    let status = match q.status.as_deref() {
        None | Some("") => None,
        Some(s) => Some(Status::parse(s).ok_or(StatusCode::BAD_REQUEST)?),
    };
    let limit = q.limit.clamp(1, 200);
    let offset = q.offset.max(0);
    let items = with_kevy(move |c| send_read::list_sends(c, &user, status, offset, limit))?;
    Ok(Json(items.into_iter().map(Into::into).collect()))
}

#[derive(Debug, serde::Deserialize)]
pub struct ShadowQuery {
    /// Sends at or after this epoch-second must have a row; anything
    /// earlier predates the row-writing and is expected to be missing.
    /// Required, because a default would decide the cutover silently and
    /// the answer depends entirely on it.
    pub since: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ShadowResponse {
    pub axis_threads: u64,
    pub projection_sends: u64,
    pub missing_before: u64,
    /// The gate. Must be zero before the Send view reads the projection.
    pub missing_since: u64,
    pub samples: Vec<String>,
}

/// `POST /api/mail/sends:shadow?since=<epoch>`
///
/// Compares the projection against the threads the old Sent axis holds.
/// Split by time on purpose: every send made before the row-writing
/// shipped has no row and always will, so counting those alongside a live
/// regression would bury it — the thread-counter work nearly lost 182
/// actionable rows inside 64 expected ones by summing them.
pub async fn shadow_sends(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(q): Query<ShadowQuery>,
) -> Result<Json<ShadowResponse>, StatusCode> {
    // The old axis lives behind the core, which owns the embedded store.
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: 2000,
            before_ts: None,
            category: None,
            domains: None,
            archived: false,
            folder: Some("Sent".to_string()),
            unread: None,
            starred: None,
            section: None,
        },
    };
    let axis: Vec<(String, i64)> = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?
        .items
        .into_iter()
        .map(|c| (c.thread_id, c.last_date))
        .collect();

    let since = q.since;
    let user_c = user.clone();
    let r = with_kevy(move |c| send_read::shadow_report(c, &user_c, &axis, since))?;
    if r.missing_since > 0 {
        tracing::warn!(
            %user, missing_since = r.missing_since, samples = ?r.samples,
            "sends since the cutover with no Send row"
        );
    }
    Ok(Json(ShadowResponse {
        axis_threads: r.axis_threads,
        projection_sends: r.projection_sends,
        missing_before: r.missing_before,
        missing_since: r.missing_since,
        samples: r.samples,
    }))
}
