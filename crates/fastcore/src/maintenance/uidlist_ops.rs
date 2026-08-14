//! Housekeeping for the maildir's UID list.
//!
//! The write path appends, so a redelivery or a sweep that raced an append
//! can leave the same message named twice. Lookups already answer with the
//! last record, so duplicates are correct and merely wasteful — this is
//! what stops the file growing forever.
//!
//! It reports `walked` as well as `dropped`, because a sweep that reports
//! only what it changed cannot tell "nothing to do" from "nothing looked
//! at" — `periodic-work-must-converge`.

use super::prelude::*;

/// `POST /v1/admin/maintenance:uidlist-compact`
pub(crate) async fn uidlist_compact_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut walked = 0u64;
    let mut dropped = 0u64;
    let mut errors = 0u64;
    let mut by_user: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for user in &users {
        match crate::uidlist::compact(user) {
            Ok((before, after)) => {
                walked += before as u64;
                dropped += (before - after) as u64;
                if before != after {
                    by_user.insert(
                        user.clone(),
                        serde_json::json!({ "before": before, "after": after }),
                    );
                }
            }
            Err(e) => {
                tracing::warn!(err = %e, %user, "uidlist compact failed");
                errors += 1;
            }
        }
    }
    Json(serde_json::json!({
        "accounts": users.len(),
        "records_walked": walked,
        "duplicates_dropped": dropped,
        "errors": errors,
        "by_user": by_user,
    }))
    .into_response()
}
