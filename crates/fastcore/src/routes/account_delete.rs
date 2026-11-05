//! Deleting an account.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use mailrs_core_sidestate::families::account_purge::{maildir, pending};

use crate::*;

/// A deleted account's address stops being a mailbox. Delivery takes any
/// maildir it finds, so the maildir is moved aside first; with it gone the
/// address is like any other, taken by an alias or catch-all or refused at
/// RCPT. The record is kept when the move fails.
///
/// The rest of what the account owns stays until the retention period
/// has passed: the address is marked pending purge, and the purge
/// (`crate::account_purge`) removes it all then, or right away under a
/// retention of 0 days.
pub(crate) async fn remove_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> axum::response::Response {
    match state.mailbox.get_account_blob(&address) {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(e) = state.mailbox.delete_account(&address) {
                tracing::warn!(err = %e, %address, "delete_account failed");
            }
            return axum::http::StatusCode::NO_CONTENT.into_response();
        }
        Err(e) => {
            tracing::error!(err = %e, %address, "reading the account failed; account kept");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    let now = now_secs();
    let mut net = match crate::account_purge::mark_conn(&state) {
        Ok(c) => c,
        Err(code) => return code.into_response(),
    };
    if let Some(c) = net.as_mut()
        && let Err(e) = pending::mark(c, &address, now)
    {
        tracing::error!(err = %e, %address, "marking the account for its purge failed; account kept");
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let root = std::path::Path::new(&state.maildir_root);
    if let Err(e) = maildir::park(root, &address, now) {
        tracing::error!(err = %e, %address, "removing the account's maildir failed; account kept");
        if let Some(c) = net.as_mut()
            && let Err(e) = pending::clear(c, &address)
        {
            tracing::error!(err = %e, %address, "the purge mark of a kept account stays; the purge skips it while it is an account");
        }
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(e) = state.mailbox.delete_account(&address) {
        tracing::warn!(err = %e, %address, "delete_account failed");
    }
    if net.is_some() && pending::retention_days() == 0 {
        crate::account_purge::purge_now(state.clone(), address).await;
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}
