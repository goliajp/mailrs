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
/// The id a session goes by in the account's list: a prefix of its
/// reference (the token's hash), never the token.
pub(crate) fn list_id(reference: &str) -> String {
    reference.chars().take(ID_LEN).collect()
}

/// [`list_id`] of the session `token` names.
pub(crate) fn list_id_of_token(token: &str) -> String {
    list_id(&session_ref(token))
}

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
                id: list_id(&reference),
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

/// The full ref and stored row of the user's session named `id`.
fn find(
    c: &mut kevy_client::Connection,
    user: &str,
    id: &str,
) -> std::io::Result<Option<(String, Vec<u8>)>> {
    if id.len() != ID_LEN {
        return Ok(None);
    }
    Ok(list_sessions(c, user)?
        .into_iter()
        .find(|(r, _)| r.starts_with(id)))
}

/// The leading part of an address: two octets of IPv4, four groups of IPv6.
fn ip_prefix(ip: &str) -> String {
    match ip.contains(':') {
        true => ip.split(':').take(4).collect::<Vec<_>>().join(":"),
        false => ip.split('.').take(2).collect::<Vec<_>>().join("."),
    }
}

/// One audit line's detail for an ended session: which it was and where it
/// came from, never anything that signs in.
fn ended_detail(reference: &str, blob: &[u8]) -> String {
    let s: Stored = serde_json::from_slice(blob).unwrap_or_default();
    let ua: String = s.user_agent.chars().take(120).collect();
    format!(
        "id={} created={} last_used={} ip={} ua={}",
        list_id(reference),
        s.created_at_unix,
        s.last_seen_unix.max(s.created_at_unix),
        ip_prefix(&s.ip),
        ua
    )
}

fn audit_ended(user: &str, reference: &str, blob: &[u8]) {
    let id = list_id(reference);
    crate::handlers::audit::record(
        user,
        "auth.session.end",
        &id,
        &ended_detail(reference, blob),
    );
}

/// DELETE /api/auth/sessions/{id} — end one of the user's sessions.
pub async fn end(
    State(_state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> StatusCode {
    let who = user.clone();
    let ended = with_kevy(move |c| match find(c, &who, &id)? {
        Some((reference, blob)) => {
            let gone = end_session_by_ref(c, &who, &reference)?;
            Ok(gone.then_some((reference, blob)))
        }
        None => Ok(None),
    });
    match ended {
        Ok(Some((reference, blob))) => {
            audit_ended(&user, &reference, &blob);
            StatusCode::NO_CONTENT
        }
        Ok(None) => StatusCode::NOT_FOUND,
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
        let mut gone = Vec::new();
        for (reference, blob) in list_sessions(c, &who)? {
            if reference != current && end_session_by_ref(c, &who, &reference)? {
                gone.push((reference, blob));
            }
        }
        Ok(gone)
    })?;
    // one line per session, so whose sessions these were can be told later
    for (reference, blob) in &ended {
        audit_ended(&user, reference, blob);
    }
    crate::handlers::audit::record(
        &user,
        "auth.session.end_others",
        &ended.len().to_string(),
        "",
    );
    Ok(Json(serde_json::json!({ "ended": ended.len() })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ended_session_is_described_without_its_token() {
        let blob = serde_json::to_vec(&Stored {
            created_at_unix: 100,
            last_seen_unix: 200,
            ip: "114.17.3.4".into(),
            user_agent: "curl/8.7.1".into(),
        })
        .unwrap();
        let reference = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            ended_detail(reference, &blob),
            "id=0123456789abcdef created=100 last_used=200 ip=114.17 ua=curl/8.7.1"
        );
        assert_eq!(ip_prefix("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2");
    }
}
