//! Handlers for `mailrs_core_api::method::mailbox`: a user's folders.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::mailbox as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/mailboxes
pub async fn list_mailboxes(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::ListMailboxesResponse>, StatusCode> {
    let rows = state.mailbox.list_mailboxes(&user).await.map_err(|e| {
        tracing::warn!(error = %e, user = %user, "list_mailboxes failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let mut items: Vec<wire::MailboxWire> = rows.iter().map(Into::into).collect();
    items.sort_by_key(|m| (folder_rank(&m.name), m.name.clone()));
    Ok(Json(wire::ListMailboxesResponse { items }))
}

/// The default folders first, in the order the kevy core lists them, then
/// the user's own by name.
fn folder_rank(name: &str) -> usize {
    const DEFAULTS: [&str; 5] = ["INBOX", "Sent", "Drafts", "Junk", "Trash"];
    DEFAULTS
        .iter()
        .position(|d| *d == name)
        .unwrap_or(DEFAULTS.len())
}
