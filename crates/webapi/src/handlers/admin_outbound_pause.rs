//! The accounts whose outbound mail is paused, and releasing them. Super
//! administrators only (`kevy_util::SUPER_ONLY`): the list names whose
//! mail is held, and releasing sends or discards it.

use axum::Json;
use axum::extract::{Extension, Path};
use axum::http::StatusCode;
use mailrs_core_sidestate::families::outbound::pause;

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

/// GET /api/admin/outbound/paused
pub async fn list_paused() -> Result<Json<serde_json::Value>, StatusCode> {
    let items = with_kevy(pause::paused)?;
    Ok(Json(serde_json::json!({ "items": items })))
}

#[derive(Debug, serde::Deserialize)]
pub struct ResumeRequest {
    /// What becomes of the held mail.
    pub held: HeldChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeldChoice {
    Send,
    Discard,
}

/// POST /api/admin/outbound/paused/{address}/resume — lift the pause; the
/// held mail is sent or discarded, and the answer counts each. 404 when
/// the account is not paused.
pub async fn resume(
    _: crate::session::confirm::Confirmed,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Path(address): Path<String>,
    Json(req): Json<ResumeRequest>,
) -> Result<Json<pause::Resumed>, StatusCode> {
    let send = req.held == HeldChoice::Send;
    let addr = address.to_ascii_lowercase();
    let now = chrono::Utc::now().timestamp();
    let done =
        with_kevy(move |c| pause::resume(c, &addr, send, now))?.ok_or(StatusCode::NOT_FOUND)?;
    let detail = format!("sent={} discarded={}", done.sent, done.discarded);
    crate::handlers::audit::record(&actor, "outbound.resume", &address, &detail);
    Ok(Json(done))
}
