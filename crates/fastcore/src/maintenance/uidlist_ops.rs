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

/// `POST /v1/admin/maintenance:uidlist-backfill`
///
/// Write each mailbox's uidlist from the UIDs its index already holds.
///
/// **Without this the file only ever describes mail that arrives after the
/// deploy.** The write path appends on delivery and the self-heal adopts
/// what the file names, but neither touches a message that is already
/// indexed and already healed — on production that is every one of 32,000
/// messages, so the rebuild story would cover none of the mailbox it was
/// written for. This is the one-time bridge from the index to the file.
///
/// The direction is the opposite of the steady-state rule (tier 1 wins)
/// and deliberately so: the file does not exist yet, so the index is the
/// only record of a promise already made to clients. Once written, the
/// file is the authority and this route has nothing left to do.
///
/// A UID the file **already** names is left alone, so this cannot rewrite
/// a promise — it only fills gaps, and a second run reports `added: 0`.
pub(crate) async fn uidlist_backfill_route(
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
    let mut added = 0u64;
    let mut no_uid = 0u64;
    let mut no_file = 0u64;
    let mut errors = 0u64;
    let mut by_user: std::collections::BTreeMap<String, serde_json::Value> = Default::default();

    for user in &users {
        let mut u_walked = 0u64;
        let mut u_added = 0u64;
        let known = crate::uidlist::load(user);
        let mut fresh: Vec<(u32, String)> = Vec::new();
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                    continue;
                };
                u_walked += 1;
                if facts.uid == 0 {
                    // An import whose uid was never allocated. Nothing to
                    // promise, so nothing to record — and inventing one
                    // here would be a promise no client has been given.
                    no_uid += 1;
                    continue;
                }
                if facts.blob_ref.is_empty() {
                    no_file += 1;
                    continue;
                }
                if known
                    .as_ref()
                    .and_then(|l| l.uid_of(&facts.blob_ref))
                    .is_some()
                {
                    continue;
                }
                fresh.push((facts.uid, facts.blob_ref));
                u_added += 1;
            }
        }
        walked += u_walked;
        added += u_added;
        if u_added > 0 {
            if let Err(e) = crate::uidlist::extend(user, &fresh) {
                tracing::warn!(err = %e, %user, "uidlist backfill failed");
                errors += 1;
                continue;
            }
            by_user.insert(
                user.clone(),
                serde_json::json!({ "walked": u_walked, "added": u_added }),
            );
        }
    }

    Json(serde_json::json!({
        "accounts": users.len(),
        "messages_walked": walked,
        "records_added": added,
        "skipped_no_uid": no_uid,
        "skipped_no_file": no_file,
        "errors": errors,
        "by_user": by_user,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:allocate-missing-uids`
///
/// Give a UID to every per-user row that has none.
///
/// A message with `uid: 0` cannot be fetched by UID, and that is not
/// cosmetic: viewing the source and downloading any attachment both go
/// through `by-uid`, which answers 404 for it, and the web client uses the
/// UID as the timeline's React key — a repeated zero in one thread is the
/// duplicate-bubble defect of 2026-07-08. Measured on production: 215 such
/// rows, all imports whose UID was never allocated.
///
/// Safe precisely because there is nothing to break: no client has ever
/// been given a number for these messages. The store's guard refuses to
/// change a UID that exists, so this can only fill holes, and the new
/// numbers go into the maildir's uidlist in the same pass — a promise made
/// only in the index is one a rebuild would forget.
pub(crate) async fn allocate_missing_uids_route(
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
    let mut allocated = 0u64;
    let mut errors = 0u64;
    let mut by_user: std::collections::BTreeMap<String, u64> = Default::default();

    for user in &users {
        let mut fresh: Vec<(u32, String)> = Vec::new();
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            for mid in state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default()
            {
                let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
                    continue;
                };
                walked += 1;
                if facts.uid != 0 {
                    continue;
                }
                let Ok(uid) = state.mailbox.allocate_uid(user, &mid) else {
                    errors += 1;
                    continue;
                };
                match state.mailbox.set_user_message_uid_if_unset(user, &mid, uid) {
                    Ok(Some(blob_ref)) => {
                        allocated += 1;
                        *by_user.entry(user.clone()).or_default() += 1;
                        if !blob_ref.is_empty() {
                            fresh.push((uid, blob_ref));
                        }
                    }
                    // Raced, or the row went away. `allocate_uid` is keyed
                    // by message id and idempotent, so nothing leaked.
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(err = %e, %user, %mid, "uid fill failed");
                        errors += 1;
                    }
                }
            }
        }
        if !fresh.is_empty()
            && let Err(e) = crate::uidlist::extend(user, &fresh)
        {
            tracing::warn!(err = %e, %user, "uidlist extend after allocation failed");
            errors += 1;
        }
    }

    Json(serde_json::json!({
        "accounts": users.len(),
        "messages_walked": walked,
        "uids_allocated": allocated,
        "errors": errors,
        "by_user": by_user,
    }))
    .into_response()
}

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
