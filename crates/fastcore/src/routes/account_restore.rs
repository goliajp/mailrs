//! Restoring a deleted account before its purge.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use mailrs_core_api::method::admin::{RestoreAccountRequest, RestoreRefusal, RestoreRefused};
use mailrs_core_sidestate::families::account_purge::{pending, restore};

use super::account_delete::Record;
use crate::*;

fn refuse(r: RestoreRefused) -> axum::response::Response {
    (StatusCode::CONFLICT, Json(r)).into_response()
}

fn failed(e: std::io::Error, address: &str, what: &str) -> axum::response::Response {
    tracing::error!(err = %e, %address, "restoring the account: {what} failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `POST /v1/admin/accounts/{address}/restore`: the account record the
/// delete kept beside its purge mark goes back, then the parked maildir,
/// then the mark goes, with an audit row. Everything else the account
/// owned never left: the purge had not run.
pub(crate) async fn restore_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
    Json(req): Json<RestoreAccountRequest>,
) -> axum::response::Response {
    let Some(mut net) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let live = match state.mailbox.get_account_blob(&address) {
        Ok(b) => b.is_some(),
        Err(e) => return failed(e, &address, "reading the account"),
    };
    let root = std::path::Path::new(&state.maildir_root);
    let dir = match restore::check(&mut net, root, &address, live, req.parked.as_deref()) {
        Ok(Ok(dir)) => dir,
        Ok(Err(r)) => return refuse(r),
        Err(e) => return failed(e, &address, "checking"),
    };
    let record = match pending::record(&mut net, &address) {
        Ok(Some(r)) => serde_json::from_str::<Record>(&r).ok(),
        Ok(None) => None,
        Err(e) => return failed(e, &address, "reading the kept record"),
    };
    let Some(record) = record else {
        return refuse(RestoreRefused {
            refused: RestoreRefusal::NoRecord,
            parked: Vec::new(),
        });
    };
    if let Err(e) = state.mailbox.upsert_account(&address, &record.account) {
        return failed(e, &address, "writing the account");
    }
    if let Some(perms) = &record.perms
        && let Err(e) = state.mailbox.upsert_permissions(&address, perms)
    {
        return failed(e, &address, "writing the permissions");
    }
    let back = match restore::finish(
        &mut net,
        root,
        &address,
        dir.as_deref(),
        &req.by,
        now_secs(),
    ) {
        Ok(back) => back,
        Err(e) => return failed(e, &address, "moving the maildir back"),
    };
    tracing::info!(%address, by = %req.by, rescheduled = back.rescheduled,
        drafts = back.returned_as_drafts, "deleted account restored");
    Json(back).into_response()
}
