//! Deleting an account.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;

use crate::*;

/// A deleted account's address stops being a mailbox. Delivery takes any
/// maildir it finds, so the maildir is moved aside first; with it gone the
/// address is like any other, taken by an alias or catch-all or refused at
/// RCPT. The record is kept when the move fails.
pub(crate) async fn remove_account_route(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> axum::response::Response {
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());
    let set_aside = match state.mailbox.get_account_blob(&address) {
        Ok(Some(_)) => set_aside_maildir(&root, &address, now_secs()),
        Ok(None) => Ok(()),
        Err(e) => Err(e),
    };
    if let Err(e) = set_aside {
        tracing::error!(err = %e, %address, "removing the account's maildir failed; account kept");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(e) = state.mailbox.delete_account(&address) {
        tracing::warn!(err = %e, %address, "delete_account failed");
    }
    axum::http::StatusCode::NO_CONTENT.into_response()
}

/// `<root>/<domain>/<local>` to `<root>/.deleted/<domain>/<local>.<ts>`,
/// where an admin can move it back. No maildir is nothing to do.
fn set_aside_maildir(root: &str, address: &str, ts: i64) -> std::io::Result<()> {
    let Some((local, domain)) = address.split_once('@') else {
        return Ok(());
    };
    let root = std::path::Path::new(root);
    let from = root.join(domain).join(local);
    match std::fs::symlink_metadata(&from) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    let into = root.join(".deleted").join(domain);
    std::fs::create_dir_all(&into)?;
    std::fs::rename(&from, into.join(format!("{local}.{ts}")))?;
    tracing::info!(%address, to = %into.display(), "deleted account's maildir set aside");
    Ok(())
}
