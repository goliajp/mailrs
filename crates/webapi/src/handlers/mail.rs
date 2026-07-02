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
///
/// `unseen` count for INBOX comes from `zcard mailrs:user:<u>:threads:has_unread`
/// on the network kevy — that zset is the same one the "has_unread"
/// filter queries. Other folders read 0 (Sent/Drafts/Trash aren't
/// tracked by their own has_unread zset yet).
pub async fn get_folders(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<FolderInfo>>, StatusCode> {
    let resp = state.fast().list_mailboxes(&user).await.map_err(map_err)?;
    let inbox_unseen = state
        .fast()
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

// ── reactions (network kevy) ───────────────────────────────────────
//
// Keys:
//   reactions:<tid>:<uid>              hash: emoji -> comma-joined user list
//
// Aggregated view is computed at read time; toggle mutates the set.

/// GET /api/conversations/{tid}/reactions — aggregate across all
/// messages in the thread.
///
/// Reads the per-thread `reactions_index:<tid>` set (maintained by
/// `toggle_reaction`); for each member `<uid>` it HGETALLs the
/// `reactions:<tid>:<uid>` hash and folds the emoji → user CSV pairs
/// into aggregated rows with `count` and `me` flag from `user`'s
/// membership.
pub async fn get_thread_reactions(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(thread_id): Path<String>,
) -> Result<Json<mailrs_core_api::method::admin::ReactionsResponse>, StatusCode> {
    let index_key = format!("reactions_index:{thread_id}");
    let members = with_kevy(move |c| c.smembers(index_key.as_bytes()))?;
    let mut reactions = Vec::new();
    for uid_bytes in members {
        let uid_s = String::from_utf8_lossy(&uid_bytes).to_string();
        let Ok(uid) = uid_s.parse::<i64>() else {
            continue;
        };
        let key = format!("reactions:{thread_id}:{uid}");
        let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
        let mut i = 0;
        while i + 1 < flat.len() {
            let emoji = String::from_utf8_lossy(&flat[i]).to_string();
            let users_str = String::from_utf8_lossy(&flat[i + 1]);
            let users: Vec<&str> = users_str.split(',').filter(|s| !s.is_empty()).collect();
            let count = users.len() as i64;
            let me = users.iter().any(|u| *u == user);
            reactions.push(mailrs_core_api::method::admin::ReactionAggregateRow {
                message_uid: uid,
                emoji,
                count,
                me,
            });
            i += 2;
        }
    }
    Ok(Json(mailrs_core_api::method::admin::ReactionsResponse {
        reactions,
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
    let index_key = format!("reactions_index:{thread_id}");
    let emoji = req.emoji.clone();
    let user_c = user.clone();
    let key_c = key.clone();
    let emoji_c = emoji.clone();
    let index_key_c = index_key.clone();
    let uid_bytes = uid.to_string();
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
        // Keep the per-thread index in sync: the message contributes to
        // the aggregate iff its hash still has ≥ 1 emoji entry after the
        // toggle. Cheap probe: `hlen`.
        let remaining = c.hlen(key_c.as_bytes())?;
        if remaining > 0 {
            c.sadd(index_key_c.as_bytes(), &[uid_bytes.as_bytes()])?;
        } else {
            c.srem(index_key_c.as_bytes(), &[uid_bytes.as_bytes()])?;
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
