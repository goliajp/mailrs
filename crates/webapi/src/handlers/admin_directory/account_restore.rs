//! Restoring a deleted account before its purge.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use mailrs_core_api::error::CoreApiError;
use mailrs_core_api::method::admin as wire;

use crate::WebState;
use crate::handlers::admin::map_err;
use crate::handlers::conversations::AuthedUser;

#[derive(Debug, Default, serde::Deserialize)]
pub struct RestoreAccountBody {
    /// Which parked maildir, by the delete time its 409 lists; needed only
    /// for an address deleted more than once.
    #[serde(default)]
    pub parked: Option<String>,
}

/// What the core said when it refused, `{refused, parked}`, as it said it.
fn refusal(message: &str) -> axum::response::Response {
    let body = message
        .split_once(": ")
        .and_then(|(_, json)| serde_json::from_str::<wire::RestoreRefused>(json).ok());
    match body {
        Some(b) => (StatusCode::CONFLICT, Json(b)).into_response(),
        None => StatusCode::CONFLICT.into_response(),
    }
}

/// POST /api/admin/accounts/{address}/restore — a deleted account whose
/// purge has not run, back with its mail, settings and memberships. Super
/// administrator (declared in `route_permissions`) with a recent password
/// confirmation. The core writes the `account.restore` audit row, naming
/// the caller; 409 says why it refused and lists the parked maildirs.
pub async fn restore_account(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    body: Option<Json<RestoreAccountBody>>,
) -> Result<axum::response::Response, StatusCode> {
    let req = wire::RestoreAccountRequest {
        by: actor,
        parked: body.and_then(|Json(b)| b.parked),
    };
    match state.core.restore_account(&address, &req).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(CoreApiError::Conflict(message)) => Ok(refusal(&message)),
        Err(e) => Err(map_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_passes_the_cores_reason_on() {
        let res = refusal(r#"restore_account: {"refused":"several_parked","parked":["1","2"]}"#);
        assert_eq!(res.status(), StatusCode::CONFLICT);
        assert_eq!(refusal("restore_account").status(), StatusCode::CONFLICT);
    }
}
