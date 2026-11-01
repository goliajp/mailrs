//! `POST /v1/users/{user}/senders:accept`: the acceptance recorded on each
//! contact (the network store both cores' delivery reads) and as this
//! core's own fact, which its section query reads.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mailrs_core_api::method::senders::{AcceptSendersRequest, AcceptSendersResponse, normalized};

use crate::core_rpc::CoreRpcState;

pub async fn accept_senders(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<AcceptSendersRequest>,
) -> Result<Json<AcceptSendersResponse>, StatusCode> {
    let addresses = normalized(&req.addresses);
    let Some(url) = state.net_url.clone() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let (who, list, at) = (user.clone(), addresses.clone(), req.accepted_at);
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let mut conn = kevy_client::Connection::connect(&url)?;
        for address in &list {
            mailrs_core_sidestate::families::contacts::accept(&mut conn, &who, address, at)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "accepting senders panicked");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .map_err(|e| {
        tracing::error!(error = %e, %user, "accepting a sender on the contact failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    state
        .mailbox
        .accept_senders(&user, &addresses, req.accepted_at)
        .await
        .map_err(super::failed("accept_senders", &user))?;
    Ok(Json(AcceptSendersResponse {
        accepted: addresses.len() as u32,
    }))
}
