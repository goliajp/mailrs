//! The signed-in user's sessions: where each was signed in from and last
//! used, and ending one or all the others.
//!
//! A session is named by the first characters of its ref — a hash of its
//! token — which says which one it is and cannot be used to sign in.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use mailrs_core_sidestate::keys::session::{end_session_by_ref, list_sessions, session_ref};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// Characters of the ref a session is shown and named by.
const ID_LEN: usize = 16;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Stored {
    #[serde(default)]
    created_at_unix: i64,
    #[serde(default)]
    last_seen_unix: i64,
    #[serde(default)]
    ip: String,
    #[serde(default)]
    user_agent: String,
}

#[derive(serde::Serialize)]
pub struct SessionView {
    pub id: String,
    pub created_at: i64,
    /// When it was last used, to ten minutes; its creation when never since.
    pub last_seen: i64,
    pub ip: String,
    pub user_agent: String,
    /// The session this request came with.
    pub current: bool,
}

fn current_ref(headers: &HeaderMap) -> Option<String> {
    crate::session::session_token(headers).map(|t| session_ref(&t))
}

/// GET /api/auth/sessions — most recently used first.
pub async fn list(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
) -> Result<Json<Vec<SessionView>>, StatusCode> {
    let current = current_ref(&headers).unwrap_or_default();
    let rows = with_kevy(move |c| list_sessions(c, &user))?;
    let mut out: Vec<SessionView> = rows
        .into_iter()
        .map(|(reference, blob)| {
            let s: Stored = serde_json::from_slice(&blob).unwrap_or_default();
            SessionView {
                id: reference.chars().take(ID_LEN).collect(),
                created_at: s.created_at_unix,
                last_seen: s.last_seen_unix.max(s.created_at_unix),
                ip: s.ip,
                user_agent: s.user_agent,
                current: reference == current,
            }
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.last_seen));
    Ok(Json(out))
}

/// The full ref of the user's session named `id`.
fn find(c: &mut kevy_client::Connection, user: &str, id: &str) -> std::io::Result<Option<String>> {
    if id.len() != ID_LEN {
        return Ok(None);
    }
    Ok(list_sessions(c, user)?
        .into_iter()
        .map(|(r, _)| r)
        .find(|r| r.starts_with(id)))
}

/// DELETE /api/auth/sessions/{id} — end one of the user's sessions.
pub async fn end(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> StatusCode {
    let who = user.clone();
    let ended = with_kevy(move |c| match find(c, &who, &id)? {
        Some(reference) => end_session_by_ref(c, &who, &reference),
        None => Ok(false),
    });
    match ended {
        Ok(true) => {
            crate::handlers::audit::record(&user, "auth.session.end", "", "");
            StatusCode::NO_CONTENT
        }
        Ok(false) => StatusCode::NOT_FOUND,
        Err(status) => status,
    }
}

/// POST /api/auth/sessions/end-others — end every session but this one.
pub async fn end_others(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let current = current_ref(&headers).unwrap_or_default();
    let who = user.clone();
    let ended = with_kevy(move |c| {
        let mut n = 0usize;
        for (reference, _) in list_sessions(c, &who)? {
            if reference != current && end_session_by_ref(c, &who, &reference)? {
                n += 1;
            }
        }
        Ok(n)
    })?;
    crate::handlers::audit::record(&user, "auth.session.end_others", &ended.to_string(), "");
    Ok(Json(serde_json::json!({ "ended": ended })))
}
