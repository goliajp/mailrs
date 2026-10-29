//! `/api/mail/*` REST handlers — fastcore-native.
//!
//! The folder list and a folder's messages (fastcore RPC → kevy zsets).
//! Everything else lives in `handlers::prefs` or `handlers::conversations`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Wire shape UI expects from /api/mail/folders.
#[derive(serde::Serialize)]
pub struct FolderInfo {
    pub name: String,
    pub total: u32,
    pub unseen: u32,
    pub uidnext: u32,
}

/// GET /api/mail/folders/{name}/messages — thread summaries for a
/// folder (INBOX / Sent / etc). Fastcore lists via
/// `list_conversations` with the folder filter.
pub async fn list_folder_messages(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let req = mailrs_core_api::method::conversation::ListConversationsRequest {
        filter: mailrs_core_api::types::ConversationFilter {
            limit: 100,
            folder: Some(name.clone()),
            ..Default::default()
        },
    };
    let resp = state
        .core
        .list_conversations(&user, &req)
        .await
        .map_err(|e| {
            axum::http::StatusCode::from_u16(e.status_code())
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    let items: Vec<serde_json::Value> = resp
        .items
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "thread_id": c.thread_id,
                "subject": c.subject,
                "participants": c.participants,
                "message_count": c.message_count,
                "unread_count": c.unread_count,
                "last_date": c.last_date,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({
        "folder": name,
        "items": items,
    })))
}

/// GET /api/mail/folders — fastcore-native. Returns bare `FolderInfo[]`.
///
/// `unseen` for INBOX comes from core's unseen-count, which reads the
/// unread axis of the declared `threaduser` table — the same answer
/// the "has_unread" filter gets. Other folders read 0 (Sent/Drafts/
/// Trash have no unread axis of their own).
pub async fn get_folders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<FolderInfo>>, StatusCode> {
    let resp = state.core.list_mailboxes(&user).await.map_err(map_err)?;
    let inbox_unseen = state
        .core
        .unseen_count(&user)
        .await
        .map(|r| r.count)
        .unwrap_or(0);
    let folders = resp
        .items
        .into_iter()
        .map(|m| {
            let unseen = if m.name.eq_ignore_ascii_case("INBOX") {
                inbox_unseen.max(0) as u32
            } else {
                0
            };
            FolderInfo {
                name: m.name,
                total: m.uidnext.saturating_sub(1),
                unseen,
                uidnext: m.uidnext,
            }
        })
        .collect();
    Ok(Json(folders))
}
