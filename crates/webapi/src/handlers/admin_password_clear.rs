//! Clearing passwords, for a super administrator. What a clearing does and
//! what the preview reports is `mailrs_core_sidestate::families::password_clear`,
//! shared with the operator's way in on the host.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use mailrs_core_api::method::admin::ClearPasswordRequest;
use mailrs_core_sidestate::families::password_clear::{
    self as pc, Cleared, Credentials, Preview, PreviewRow, Refused,
};

use crate::WebState;
use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

pub use pc::{Addresses, MAX_ACCOUNTS, issuers};

fn addresses(req: Addresses) -> Result<Vec<String>, StatusCode> {
    pc::addresses(req).map_err(|r| match r {
        Refused::None => StatusCode::BAD_REQUEST,
        Refused::TooMany => StatusCode::PAYLOAD_TOO_LARGE,
    })
}

/// POST /api/admin/password-clear/preview — super administrators only (the
/// admin middleware), with a recent confirmation.
pub async fn preview(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Json(req): Json<Addresses>,
) -> Result<Json<Preview>, StatusCode> {
    let mut items = Vec::new();
    for address in addresses(req)? {
        items.push(row(&state, address).await?);
    }
    Ok(Json(Preview {
        items,
        issuers_configured: !issuers().is_empty(),
    }))
}

async fn row(state: &WebState, address: String) -> Result<PreviewRow, StatusCode> {
    let account = match state.core.get_account_with_hash(&address).await {
        Ok(a) => Some(a.public.password_cleared_at),
        Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => None,
        Err(e) => return Err(crate::handlers::admin::map_err(e)),
    };
    if account.is_none() {
        return Ok(pc::row(address, None, Credentials::default(), 0));
    }
    let addr = address.clone();
    let creds = with_kevy(move |c| pc::credentials(c, &addr))?;
    let now = crate::handlers::complete::now_secs();
    let smtp = pc::smtp_logins(crate::login_guard::GUARD.as_ref(), &address, now)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "password clear preview: sign-in events unreachable");
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    Ok(pc::row(address, account, creds, smtp))
}

/// POST /api/admin/password-clear — clear each account's password, end its
/// sessions, and close its signed-in protocol connections.
pub async fn clear(
    _: crate::session::confirm::Confirmed,
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(actor)): Extension<AuthedUser>,
    Json(req): Json<Addresses>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut items: Vec<Cleared> = Vec::new();
    for address in addresses(req)? {
        let by = ClearPasswordRequest { by: actor.clone() };
        match state.core.clear_account_password(&address, &by).await {
            Ok(()) => {}
            Err(mailrs_core_api::error::CoreApiError::NotFound(_)) => {
                items.push(Cleared::unknown(address));
                continue;
            }
            Err(e) => return Err(crate::handlers::admin::map_err(e)),
        }
        let (who, addr) = (actor.clone(), address.clone());
        let now = crate::handlers::complete::now_secs();
        items.push(with_kevy(move |c| pc::after_clear(c, &who, &addr, now))?);
    }
    Ok(Json(serde_json::json!({ "items": items })))
}
