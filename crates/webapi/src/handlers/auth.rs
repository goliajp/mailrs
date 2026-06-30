//! `/api/auth/*` REST handlers (currently just `auth_me`).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use serde::Serialize;

use crate::WebState;
use crate::handlers::conversations::{AuthedDisplayName, AuthedUser};

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
        .core_client
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
