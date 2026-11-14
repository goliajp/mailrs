//! The fraud verdict a message arrived with, and whether a recipient takes
//! mail.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::core_rpc::CoreRpcState;

/// GET /v1/admin/recipients/{address}/deliverable — whether this core would
/// take mail for the address. A store that cannot be read is a 500, which
/// the receiver treats as "accept": refusing a real address because the
/// database was away is the failure this check must not have.
pub async fn recipient_deliverable(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<mailrs_core_api::method::admin::RecipientDeliverableResponse>, StatusCode> {
    let fwd = address.trim().to_ascii_lowercase();
    if mailrs_core_sidestate::families::mock::is_mock(&fwd) {
        return Ok(Json(
            mailrs_core_api::method::admin::RecipientDeliverableResponse { deliverable: true },
        ));
    }
    let dir = Arc::new(crate::intake::PgDirectory::new(
        state.domain.clone(),
        state.maildir_root.clone(),
        state.net_url.clone(),
    ));
    let route = dir.route(address).await.map_err(|e| {
        tracing::warn!(error = %e, "recipient check: a store could not be read");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let deliverable = !matches!(route, mailrs_intake::route::Route::Nobody);
    Ok(Json(
        mailrs_core_api::method::admin::RecipientDeliverableResponse { deliverable },
    ))
}
