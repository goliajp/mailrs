//! Shared kevy connection helper. Every handler in this crate that
//! reads or writes the network kevy at `MAILRS_KEVY_URL` used to carry
//! its own copy of `with_kevy` — 6 near-duplicates. This module owns
//! the single definition so callers just `use crate::handlers::kevy_util::with_kevy`.
//!
//! The helper spawns a blocking OS thread and opens a fresh connection
//! per call. Chose OS-thread over `tokio::task::spawn_blocking` because
//! `kevy_client::Connection` is `!Send` on some platforms and we want
//! this to work in every async context.

use std::sync::Arc;

use axum::http::StatusCode;

use crate::WebState;

/// Guard for every `/api/admin/*` handler: return 403 unless the
/// caller's effective_permissions grant admin authority.
///
/// Admin authority = `is_super == true` OR any permission string that
/// starts with `admin.` (e.g. `admin.accounts`, `admin.domains`,
/// `admin.groups` — matches the monolith's permission model at
/// `crates/server/src/permission.rs`).
///
/// Handlers that need a more specific permission (e.g. only
/// `admin.impersonate` for audit endpoints) can layer on top with
/// [`require_permission`].
pub async fn require_admin(state: &Arc<WebState>, user: &str) -> Result<(), StatusCode> {
    let perms = state
        .fast()
        .effective_permissions(user)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if perms.is_super || perms.permissions.iter().any(|p| p.starts_with("admin.")) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Guard for a specific permission string. Used by handlers whose
/// action maps to a single well-known permission (e.g. `admin.export`
/// wants `admin.impersonate`).
pub async fn require_permission(
    state: &Arc<WebState>,
    user: &str,
    permission: &str,
) -> Result<(), StatusCode> {
    let perms = state
        .fast()
        .effective_permissions(user)
        .await
        .map_err(|_| StatusCode::FORBIDDEN)?;
    if perms.is_super || perms.permissions.iter().any(|p| p == permission) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Axum middleware — enforce admin authority on any `/api/admin/*`
/// route. Runs after `session_auth_middleware` so it can pull the
/// authed user out of request extensions.
pub async fn admin_middleware(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let path = req.uri().path().to_string();
    let requires_admin = path.starts_with("/api/admin/")
        || path == "/api/admin"
        || path.starts_with("/oauth/") // oidc provider ops on client credentials
        || path == "/api/admin/export";
    if !requires_admin {
        return next.run(req).await;
    }
    let user = req
        .extensions_mut()
        .get::<crate::handlers::conversations::AuthedUser>()
        .map(|u| u.0.clone());
    let Some(user) = user else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Err(status) = require_admin(&state, &user).await {
        return status.into_response();
    }
    next.run(req).await
}

/// Run `f` against a fresh kevy connection on a blocking thread.
/// Any I/O error surfaces as `INTERNAL_SERVER_ERROR`. Callers that
/// need to distinguish (e.g., NOT_FOUND on empty key) should peek the
/// returned value before mapping.
pub fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
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
