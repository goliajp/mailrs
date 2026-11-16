//! The sent list, finding a thread by Message-ID, and marking a whole
//! view as read.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::thread as wire;
use mailrs_mailbox::pg::{Held, ThreadQuery};

use crate::core_rpc::CoreRpcState;
use crate::core_rpc::handlers::conversation::{labelled_query, thread_query};
use crate::core_rpc::handlers::failed;

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
/// list shows, through the list's own query: [`thread_query`], a label's
/// conversations by [`labelled_query`], or Review as `list_quarantined`
/// reads it. Every row, not one page.
pub async fn mark_list_read(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::MarkListReadRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let whole = req.whole_list();
    let labelled = match (req.quarantined, whole.label.as_deref()) {
        (false, Some(id)) => Some(state.mailbox.threads_with_label(&user, id).await.map_err(
            |e| {
                tracing::warn!(error = %e, %user, "mark_list_read: threads_with_label failed");
                StatusCode::INTERNAL_SERVER_ERROR
            },
        )?),
        _ => None,
    };
    let q = match (req.quarantined, labelled.as_deref()) {
        (true, _) => ThreadQuery {
            held: Held::Only,
            ..ThreadQuery::all(&user)
        },
        (false, Some(ids)) => labelled_query(thread_query(&user, &whole), ids),
        (false, None) => thread_query(&user, &whole),
    };
    let q = ThreadQuery { limit: None, ..q };
    let flipped = state
        .mailbox
        .mark_rows_read(&q)
        .await
        .map_err(failed("mark_list_read", &user))?;
    Ok(Json(serde_json::json!({ "ok": true, "flipped": flipped })))
}
