//! Backfills over the per-user message projection
//! (`.claude/rfcs/20260731-per-user-message-projection.md`).

use super::prelude::*;

/// `POST /v1/admin/maintenance:backfill-user-messages` — stage 2 of the/// `POST /v1/admin/maintenance:backfill-user-messages` — stage 2 of the
/// per-user message projection.
///
/// For every (user, thread) row, decide for each message in the shared
/// thread index whether that user has their own copy, and record their
/// filename if so. A message the user never received gets no row, which is
/// the correction: `thread:{tid}:messages` is shared, so today every owner
/// of a thread is served every message in it whoever it was delivered to.
///
/// Idempotent — rerunning re-derives the same rows from the same files.
///
/// See `.claude/rfcs/20260731-per-user-message-projection.md`.
pub(crate) async fn backfill_user_messages_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use mailrs_core_api::method::message::MessageWire;

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let root = std::env::var("MAILRS_MAILDIR").unwrap_or_else(|_| "/data/maildir".into());

    let mut threads_seen = 0u64;
    let mut messages_seen = 0u64;
    let mut rows_written = 0u64;
    let mut not_this_users = 0u64;
    let mut no_message_id = 0u64;
    // Per user, capped per user: a global cap fills from whichever account
    // the walk reaches first and then describes only that one.
    let mut samples: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for user in &users {
        let by_mid = user_files_by_message_id(&root, user);
        let tids = state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default();
        for tid in tids {
            threads_seen += 1;
            for blob in state
                .mailbox
                .thread_messages_for_maintenance(&tid)
                .unwrap_or_default()
            {
                let Ok(w) = serde_json::from_slice::<MessageWire>(&blob) else {
                    continue;
                };
                messages_seen += 1;
                if w.message_id.is_empty() {
                    no_message_id += 1;
                    continue;
                }
                // Two ways a message can be this user's, in order of
                // directness.
                //
                // 1. The stored `blob_ref` resolves inside this user's own
                //    maildir. Then it *is* their copy and nothing else needs
                //    checking — this is the case for every message on a
                //    single-owner thread, and it does not depend on the
                //    Message-ID at all.
                // 2. It does not, so look for their own copy under a
                //    different filename, by Message-ID.
                //
                // The first version had only step 2, and skipped any file
                // whose `Message-ID` header is absent — mail whose stored id
                // was synthesised as `{maildir_id}@mailrs.local` by the
                // pg-lane reconcile. 113 of `lihao@golia.jp`'s threads read
                // as "user has no copy" with the file sitting in their
                // maildir, and a cutover on that reading would have blanked
                // every one of them. Readability is the question; a name
                // matching is only one way of answering it.
                let own = match read_maildir_file(user, &w.blob_ref).is_some() {
                    true => Some(w.blob_ref.clone()),
                    false => by_mid.get(&w.message_id).cloned(),
                };
                let Some(blob_ref) = own else {
                    // Neither resolves: they have no copy of this message,
                    // and the shared index was showing it to them anyway.
                    not_this_users += 1;
                    let per_user = samples.entry(user.clone()).or_default();
                    if per_user.len() < 4 {
                        per_user.push(w.message_id.clone());
                    }
                    continue;
                };
                // Their uid, not the shared blob's — which belongs to
                // whichever owner wrote it last.
                let uid = state
                    .mailbox
                    .allocate_uid(user, &w.message_id)
                    .unwrap_or(w.uid);
                if let Err(e) = state.mailbox.upsert_user_message(
                    user,
                    &tid,
                    &w.message_id,
                    w.internal_date,
                    &blob,
                    &mailrs_mailbox_kevy::UserMessageFacts {
                        blob_ref: &blob_ref,
                        uid,
                        flags: w.flags,
                        modseq: w.modseq,
                    },
                ) {
                    tracing::warn!(err = %e, %user, "backfill-user-messages: write failed");
                    continue;
                }
                rows_written += 1;
            }
        }
    }

    Json(serde_json::json!({
        "accounts": users.len(),
        "threads_seen": threads_seen,
        "messages_seen": messages_seen,
        "rows_written": rows_written,
        // Messages the shared index served to a user who never received
        // them. Non-zero is the tenancy exposure being closed, not an error.
        "not_this_users": not_this_users,
        "not_this_users_samples": samples,
        "messages_without_message_id": no_message_id,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:strip-shared-per-user-fields` — stage 6 of
/// the per-user message projection.
///
/// Stage 5 stopped writing `blob_ref` / `uid` / `flags` / `modseq` /
/// `mailbox_id` / `user_address` onto the blob every owner of a thread
/// reads. Rows written before it keep one owner's values, and this removes
/// them.
///
/// It is not a repair — nothing serves those fields since
/// `user_message_view` became the single decision, and on production none
/// of the 326 differing ones resolve for the user asking anyway
/// (`maintenance:usermsg-shadow`, `shared_resolves: 0`). It removes what a
/// future fallback could reach for, which is how the defect happened the
/// first time.
///
/// Threads are visited once even when several accounts own them: the blob
/// is shared, so stripping it per owner would rewrite the same bytes twice
/// and report double.
pub(crate) async fn strip_shared_per_user_fields_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut seen_threads: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut messages_seen = 0u64;
    let mut rewritten = 0u64;
    let mut failed = 0u64;

    for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            if !seen_threads.insert(tid.clone()) {
                continue;
            }
            match state.mailbox.strip_shared_per_user_fields(&tid) {
                Ok((seen, done)) => {
                    messages_seen += seen;
                    rewritten += done;
                }
                Err(e) => {
                    failed += 1;
                    tracing::warn!(err = %e, %tid, "strip-shared-per-user-fields failed");
                }
            }
        }
    }

    Json(serde_json::json!({
        // Accounts and threads first: `rewritten: 0` means "all clean"
        // only when the two above it are not also zero. Answering
        // `msgids_indexed: 9` against a 30,562-row table and looking
        // healthy is what made that distinction worth reporting.
        "accounts": users.len(),
        "threads_visited": seen_threads.len(),
        "messages_seen": messages_seen,
        "rewritten": rewritten,
        "threads_failed": failed,
    }))
    .into_response()
}

pub(crate) async fn backfill_thread_user_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let Some(user) = q.get("user") else {
        let users = state.mailbox.list_account_addresses().unwrap_or_default();
        return Json(serde_json::json!({ "users": users })).into_response();
    };
    let offset: i64 = q.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);

    match state.mailbox.backfill_thread_user(user, offset, limit) {
        Ok((scanned, written)) => {
            if written > 0 {
                tracing::info!(%user, offset, scanned, written, "threaduser backfill segment");
            }
            Json(serde_json::json!({
                "user": user,
                "offset": offset,
                "limit": limit,
                "scanned": scanned,
                "written": written,
                "done": (scanned as i64) < limit,
            }))
            .into_response()
        }
        Err(e) => {
            tracing::error!(err = %e, %user, "threaduser backfill failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
