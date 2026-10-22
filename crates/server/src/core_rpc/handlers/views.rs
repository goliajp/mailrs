//! Search, the sent list, finding a thread by Message-ID, and marking a
//! whole view as read.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::thread as wire;
use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_mailbox::pg::{Held, ThreadQuery};
use mailrs_mailbox::scope::{is_sent_folder, scope};

use crate::core_rpc::CoreRpcState;
use crate::core_rpc::handlers::conversation::thread_query;
use crate::core_rpc::handlers::failed;

/// How many more matches than the page to ask the text search for, so the
/// page is still full after the view's scope has dropped some.
const OVERFETCH: i64 = 8;

/// POST /v1/users/{user}/conversations:search — the matches that are in the
/// view the search was made from, best match first. Held mail is out of
/// search as it is out of every list.
pub async fn search_conversations(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::SearchConversationsRequest>,
) -> Result<Json<conv::SearchConversationsResponse>, StatusCode> {
    let limit = match req.limit {
        0 => 20,
        n => n as usize,
    };
    let ids = state
        .mailbox
        .search_thread_ids(&user, &req.query, limit as i64 * OVERFETCH)
        .await
        .map_err(failed("search", &user))?;
    let folder = req.folder.as_deref();
    let q = ThreadQuery {
        scope: scope(folder, req.category.as_deref()),
        sent: is_sent_folder(folder),
        archived: Some(req.archived),
        unread: req.unread.unwrap_or(false),
        starred: req.starred.unwrap_or(false),
        held: Held::Hide,
        thread_ids: Some(&ids),
        ..ThreadQuery::all(&user)
    };
    let mut rows = state
        .mailbox
        .thread_rows(&q)
        .await
        .map_err(failed("search", &user))?;
    let rank = |tid: &str| ids.iter().position(|i| i == tid).unwrap_or(usize::MAX);
    rows.sort_by_key(|r| rank(&r.thread_id));
    rows.truncate(limit);
    let items: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    Ok(Json(conv::SearchConversationsResponse { items }))
}

/// GET /v1/users/{user}/sent-messages — what the user sent, newest first.
pub async fn list_sent_messages(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::SentMessagesResponse>, StatusCode> {
    let rows = state
        .mailbox
        .sent_messages(&user)
        .await
        .map_err(failed("sent_messages", &user))?;
    let items = rows
        .into_iter()
        .map(
            |(uid, message_id, thread_id, to, subject, internal_date)| wire::SentMessageSummary {
                uid: uid as u32,
                message_id,
                thread_id,
                to,
                subject,
                internal_date,
            },
        )
        .collect();
    Ok(Json(wire::SentMessagesResponse { items }))
}

/// POST /v1/users/{user}/conversations:mark-all-read
pub async fn mark_all_read(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let flipped = state
        .mailbox
        .mark_all_read(&user)
        .await
        .map_err(failed("mark_all_read", &user))?;
    Ok(Json(serde_json::json!({ "ok": true, "flipped": flipped })))
}

/// POST /v1/users/{user}/conversations:mark-list-read — marks the rows the
/// list shows, through the list's own query. Every row, not one page.
pub async fn mark_list_read(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<conv::ListConversationsRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let q = ThreadQuery {
        limit: None,
        ..thread_query(&user, &req.filter)
    };
    let flipped = state
        .mailbox
        .mark_rows_read(&q)
        .await
        .map_err(failed("mark_list_read", &user))?;
    Ok(Json(serde_json::json!({ "ok": true, "flipped": flipped })))
}
