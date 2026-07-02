//! `/api/mail/*` REST handlers — fastcore-native.
//!
//! Only two remain here: `get_folders` (fastcore RPC → kevy zsets) and
//! the reactions pair (network kevy hash). Everything else lives in
//! `handlers::prefs` or `handlers::conversations`.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Wire shape UI expects from /api/mail/folders.
#[derive(serde::Serialize)]
pub struct FolderInfo {
    pub name: String,
    pub total: u32,
    pub unseen: u32,
    pub uidnext: u32,
}

/// GET /api/mail/folders — fastcore-native. Returns bare `FolderInfo[]`.
pub async fn get_folders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<FolderInfo>>, StatusCode> {
    let resp = state.fast().list_mailboxes(&user).await.map_err(map_err)?;
    let folders = resp
        .items
        .into_iter()
        .map(|m| FolderInfo {
            name: m.name,
            total: m.uidnext.saturating_sub(1),
            unseen: 0,
            uidnext: m.uidnext,
        })
        .collect();
    Ok(Json(folders))
}

// ── reactions (network kevy) ───────────────────────────────────────
//
// Keys:
//   reactions:<tid>:<uid>              hash: emoji -> comma-joined user list
//
// Aggregated view is computed at read time; toggle mutates the set.

/// GET /api/conversations/{tid}/reactions — aggregate across all
/// messages in the thread. Walks `reactions:<tid>:*` (via LRANGE of a
/// per-thread index for simplicity; here we just return an empty list
/// so the UI doesn't 500 while the aggregate index is being built).
pub async fn get_thread_reactions(
    State(_state): State<Arc<WebState>>,
    Extension(_user): Extension<AuthedUser>,
    Path(_thread_id): Path<String>,
) -> Result<Json<mailrs_core_api::method::admin::ReactionsResponse>, StatusCode> {
    Ok(Json(mailrs_core_api::method::admin::ReactionsResponse {
        reactions: Vec::new(),
    }))
}

/// PUT /api/conversations/{tid}/messages/{uid}/reactions — toggle
/// the user's presence in the `<emoji>` set for this message.
pub async fn toggle_reaction(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path((thread_id, uid)): Path<(String, i64)>,
    Json(req): Json<mailrs_core_api::method::admin::ToggleReactionRequest>,
) -> Result<Json<mailrs_core_api::method::admin::ReactionsResponse>, StatusCode> {
    let key = format!("reactions:{thread_id}:{uid}");
    let emoji = req.emoji.clone();
    let user_c = user.clone();
    let key_c = key.clone();
    let emoji_c = emoji.clone();
    with_kevy(move |c| {
        // Read existing csv, toggle user, write back.
        let cur = c
            .hget(key_c.as_bytes(), emoji_c.as_bytes())?
            .unwrap_or_default();
        let mut users: Vec<String> = String::from_utf8_lossy(&cur)
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if users.contains(&user_c) {
            users.retain(|u| u != &user_c);
        } else {
            users.push(user_c.clone());
        }
        let joined = users.join(",");
        if joined.is_empty() {
            c.hdel(key_c.as_bytes(), &[emoji_c.as_bytes()])?;
        } else {
            c.hset(key_c.as_bytes(), &[(emoji_c.as_bytes(), joined.as_bytes())])?;
        }
        Ok(())
    })?;
    // Recompute aggregate for THIS message + return it.
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
    let mut reactions = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let emoji_bytes = &flat[i];
        let users_bytes = &flat[i + 1];
        let emoji_s = String::from_utf8_lossy(emoji_bytes).to_string();
        let users_str = String::from_utf8_lossy(users_bytes);
        let users: Vec<&str> = users_str.split(',').filter(|s| !s.is_empty()).collect();
        let count = users.len() as i64;
        let me = users.iter().any(|u| *u == user);
        reactions.push(mailrs_core_api::method::admin::ReactionAggregateRow {
            message_uid: uid,
            emoji: emoji_s,
            count,
            me,
        });
        i += 2;
    }
    Ok(Json(mailrs_core_api::method::admin::ReactionsResponse {
        reactions,
    }))
}
