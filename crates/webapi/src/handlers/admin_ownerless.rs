//! Conversations nobody holds, for the super administrator: list them, and
//! delete one at a time. A delete cannot be undone, so it needs a recent
//! password confirmation and leaves one audit row per conversation.
//!
//! - `GET /api/admin/ownerless-threads`
//! - `DELETE /api/admin/ownerless-threads/{thread_id}`

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// The start of a conversation id, enough to tell rows apart in a report
/// without writing out a Message-ID.
fn id_prefix(id: &str) -> String {
    id.chars().take(8).collect()
}

/// GET /api/admin/ownerless-threads
pub async fn list(
    State(state): State<Arc<WebState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let found = state
        .core
        .ownerless_threads()
        .await
        .map_err(crate::handlers::admin::map_err)?;
    let items: Vec<serde_json::Value> = found
        .items
        .iter()
        .map(|t| {
            serde_json::json!({
                "thread_id": t.thread_id,
                "id_prefix": id_prefix(&t.thread_id),
                "messages": t.messages,
                "latest_at": t.latest_at,
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "count": items.len(), "items": items }),
    ))
}

/// DELETE /api/admin/ownerless-threads/{thread_id} — 409 when somebody
/// holds it, 404 when there is no such conversation.
pub async fn delete(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let gone = state
        .core
        .delete_ownerless_thread(&thread_id)
        .await
        .map_err(crate::handlers::admin::map_err)?;
    let detail = format!("messages={}", gone.messages);
    crate::handlers::audit::record_required(
        &actor,
        "ownerless_thread.delete",
        &thread_id,
        &detail,
    )?;
    Ok(Json(serde_json::json!({
        "thread_id": gone.thread_id,
        "messages": gone.messages,
        "latest_at": gone.latest_at,
    })))
}
