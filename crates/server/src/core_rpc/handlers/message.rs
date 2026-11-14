//! Handlers for `mailrs_core_api::method::message`: one message of a
//! user's, by uid or by Message-ID, and the invitation it carries.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
};

use crate::core_rpc::CoreRpcState;

// query_messages handler omitted from this loop — inherent method
// returns (Vec<i64>, u32 total), not Vec<MessageMeta>. Needs a separate
// wire response shape (id list + total) — implemented in next loop.

// ── flag ops (IMAP STORE) ────────────────────────────────────────────

// ── message mutate (expunge / copy / move) ──────────────────────────

/// GET /v1/users/{user}/messages/index — every message the user has, in the
/// compact form a JMAP server compares to see what changed.
pub async fn message_index(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, StatusCode> {
    use mailrs_core_api::method::message_index::{Entry, MessageIndex, respond};
    use mailrs_core_api::types::ConversationSummaryWire;
    use mailrs_mailbox::pg::{Held, ThreadQuery};
    let fail = |e: sqlx::Error| {
        tracing::warn!(error = %e, %user, "message_index failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let q = ThreadQuery {
        archived: None,
        trashed: None,
        held: Held::Hide,
        ..ThreadQuery::all(&user)
    };
    let rows = state.mailbox.thread_rows(&q).await.map_err(fail)?;
    let convs: Vec<ConversationSummaryWire> = rows.iter().map(Into::into).collect();
    let convs = super::labels::with_labels(&state, &user, convs).await;
    let msgs = state
        .mailbox
        .user_message_index(&user)
        .await
        .map_err(fail)?;
    let entries: Vec<Entry<'_>> = msgs
        .iter()
        .map(|(uid, tid, flags, sender)| Entry {
            uid: *uid,
            thread_id: tid,
            flags: *flags,
            own: mailrs_rfc5322::list_contains(sender, &user),
        })
        .collect();
    Ok(respond(
        &MessageIndex::build(&convs, &entries).encode(),
        &headers,
    ))
}
