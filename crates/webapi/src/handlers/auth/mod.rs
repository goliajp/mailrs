//! `/api/auth/*` REST handlers.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use serde::Serialize;

use crate::WebState;
use crate::handlers::conversations::{AuthedDisplayName, AuthedUser};

mod credentials;
mod session;

pub use credentials::*;
pub use session::*;

fn map_err(e: mailrs_core_api::error::CoreApiError) -> StatusCode {
    StatusCode::from_u16(e.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Shape returned by `GET /api/auth/me`. Mirrors the monolith handler
/// in `crates/server/src/web/auth/login.rs:485` so the frontend payload
/// is identical.
#[derive(Debug, Serialize)]
pub struct AuthMeResponse {
    pub address: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub accessible_domains: Vec<String>,
    pub send_as: Vec<String>,
}

/// GET /api/auth/me
pub async fn auth_me(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(address)): Extension<AuthedUser>,
    Extension(AuthedDisplayName(display_name)): Extension<AuthedDisplayName>,
) -> Result<Json<AuthMeResponse>, StatusCode> {
    let perms = state
        .core
        .effective_permissions(&address)
        .await
        .map_err(map_err)?;
    // `accessible_domains` lives in EffectivePermissions but is NOT in
    // the wire response (server cement only exposes is_super + send_as +
    // permissions on the EffectivePermissionsResponse). For Phase 3 the
    // accessible_domains field returns empty; the frontend ignores it for
    // non-admin users anyway. Full parity lands in checklist 3.20.
    Ok(Json(AuthMeResponse {
        address: perms.address.clone(),
        display_name,
        permissions: perms.permissions,
        accessible_domains: Vec::new(),
        send_as: perms.send_as,
    }))
}

#[cfg(test)]
mod pending_link_tests {
    use super::session::cookie_value;
    use axum::http::{HeaderMap, HeaderValue, header::COOKIE};

    fn headers(raw: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(COOKIE, HeaderValue::from_str(raw).expect("header"));
        h
    }

    #[test]
    fn finds_the_named_cookie_among_others() {
        let h = headers("mailrs_session=abc; mailrs_pending_link=xyz; other=1");
        assert_eq!(
            cookie_value(&h, "mailrs_pending_link").as_deref(),
            Some("xyz")
        );
        assert_eq!(cookie_value(&h, "mailrs_session").as_deref(), Some("abc"));
    }

    /// `strip_prefix` on a name alone would match any cookie that merely
    /// starts with it, so a `mailrs_pending_link_decoy` set by anything able
    /// to write a cookie would be read as the handle. The `=` is what makes
    /// it the whole name.
    #[test]
    fn a_cookie_that_merely_starts_with_the_name_is_not_it() {
        let h = headers("mailrs_pending_link_decoy=evil");
        assert_eq!(cookie_value(&h, "mailrs_pending_link"), None);
    }

    #[test]
    fn an_empty_value_is_not_a_handle() {
        let h = headers("mailrs_pending_link=");
        assert_eq!(cookie_value(&h, "mailrs_pending_link"), None);
    }

    #[test]
    fn no_cookie_header_is_no_handle() {
        assert_eq!(cookie_value(&HeaderMap::new(), "mailrs_pending_link"), None);
    }
}
