//! Deletions and compaction, behind `/v1/admin/maintenance:*`.
//!
//! Removing a structure means removing its readers **and** its writers: a
//! surviving writer refills a key nothing reads, and the census then shows
//! a healthy number for a dead axis. See
//! `.claude/rules/kevy-patterns.md` → `kevy/delete-an-index-by-its-readers`.

use super::prelude::*;

/// `POST /v1/admin/maintenance:rewrite-aof` — compact the embedded kevy
/// AOF from the CURRENT in-memory state. Recovery valve for the
/// 2026-07-17 corrupt-frame black hole: a torn frame (non-graceful
/// deploy kill) stuck mid-file meant every boot replayed only up to it
/// and appended past it — all later writes silently vanished on the
/// next restart. Rewriting emits a clean log so replay covers
/// everything again.
pub(crate) async fn rewrite_aof_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    match state.mailbox.store_ref().rewrite_aof() {
        Ok(stats) => Json(serde_json::json!({
            "ok": true,
            "stats": format!("{stats:?}"),
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "rewrite_aof failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:drop-legacy-zsets` — delete the
/// hand-maintained per-user thread indexes.
///
/// Every axis is served from the declared table and no write path
/// touches these keys, so they are dead weight held in memory. Runs
/// in-process rather than through a second store handle: opening the
/// embedded store twice replays the AOF twice and gets the container
/// OOM-killed.
///
/// `?dry=1` reports what would go without deleting it.
pub(crate) async fn drop_legacy_zsets_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let dry = q.get("dry").map(String::as_str) == Some("1");
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let store = state.mailbox.store_ref();
    let mut found = 0u64;
    let mut members = 0u64;
    let mut deleted = 0u64;
    for user in &users {
        for key in mailrs_mailbox_kevy::keys::all_user_thread_zsets(user) {
            let n = store.zcard(key.as_bytes()).unwrap_or(0);
            if n == 0 {
                continue;
            }
            found += 1;
            members += n as u64;
            if !dry {
                deleted += store.del(&[key.as_bytes()]).unwrap_or(0) as u64;
            }
        }
    }
    tracing::info!(dry, found, members, deleted, "legacy zset sweep");
    Json(serde_json::json!({
        "dry_run": dry,
        "keys_found": found,
        "members_held": members,
        "keys_deleted": deleted,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:drop-empty-threads` — remove thread rows a
/// user has no readable message in.
///
/// A thread whose messages are all gone from the user's maildir shows as a
/// row that opens onto nothing. 35 of these exist on production, all
/// `dmarc@golia.jp` mailflow probes: synthetic monitoring mail that is
/// delivered, indexed, then deleted from disk by the monitor, leaving the
/// rows behind.
///
/// Gated on the per-user index rather than on the files: a thread is empty
/// for this user when their own message index holds nothing for it, which
/// is the same question the read path asks. Threads another owner still has
/// messages in keep their row for that owner — this deletes one user's
/// membership, not the conversation.
///
/// `dry_run=1` reports without deleting, which is how it should be run
/// first.
pub(crate) async fn drop_empty_threads_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let dry_run = q.get("dry_run").map(|v| v == "1").unwrap_or(false);
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut threads_examined = 0u64;
    let mut empty = 0u64;
    let mut deleted = 0u64;
    let mut by_user: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut samples: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            threads_examined += 1;
            let mine = state
                .mailbox
                .user_thread_message_ids(user, &tid)
                .unwrap_or_default();
            if !mine.is_empty() {
                continue;
            }
            empty += 1;
            *by_user.entry(user.clone()).or_insert(0) += 1;
            let per_user = samples.entry(user.clone()).or_default();
            if per_user.len() < 4 {
                per_user.push(tid.clone());
            }
            if dry_run {
                continue;
            }
            match state.mailbox.delete_thread(user, &tid) {
                Ok((existed, _)) if existed => deleted += 1,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(err = %e, %user, %tid, "drop-empty-threads: delete failed")
                }
            }
        }
    }

    Json(serde_json::json!({
        "dry_run": dry_run,
        // What it looked at, so a zero below is legible.
        "threads_examined": threads_examined,
        "empty": empty,
        "deleted": deleted,
        "empty_by_user": by_user,
        "empty_samples": samples,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:drop-stray-usermsg-keys` — remove the
/// per-user message index keys written under the wrong prefix.
///
/// The index was first spelled `mailrs:threaduser:{user}:{tid}:messages`,
/// which sits inside the namespace `all_thread_ids_for_user` enumerates
/// with a `mailrs:threaduser:{user}:*` wildcard — so every one of them came
/// back as a thread whose id ends `:messages`. The multi-owner count went
/// from 74 to 148 the moment the backfill wrote them. The key moved to
/// `mailrs:usermsgs:{user}:{tid}`; this deletes what the first spelling
/// left behind.
///
/// Idempotent, and it reports what it scanned so a zero is legible.
pub(crate) async fn drop_stray_usermsg_keys_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();
    let (_, keys) = store.scan(0, Some(b"mailrs:threaduser:*:messages"), usize::MAX);
    let scanned = keys.len() as u64;
    let mut deleted = 0u64;
    for key in keys {
        // Belt and braces: only keys that really end with the suffix, in
        // case the glob ever matches more than intended.
        if !key.ends_with(b":messages") {
            continue;
        }
        if store.del(&[key.as_slice()]).is_ok() {
            deleted += 1;
        }
    }
    Json(serde_json::json!({
        "keys_scanned": scanned,
        "deleted": deleted,
    }))
    .into_response()
}
