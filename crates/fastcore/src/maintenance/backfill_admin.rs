//! Backfills over the admin and side-state entities — webhooks, agent
//! keys, triage, importance, contacts, the Bayes corpus, sync cursors.

use super::prelude::*;

/// `POST /v1/admin/sync/reset-cursors` — reset every registered
/// user's `mailrs:sync:cursor:<user>` key so the next
/// `ingest_sync_loop` tick treats every monolith thread as "new" and
/// runs the Group F diff path to backfill missing messages.
pub(crate) async fn reset_sync_cursors_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let addrs = match state.mailbox.list_account_addresses() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut cleared = 0u32;
    for user in &addrs {
        let key = format!("mailrs:sync:cursor:{user}");
        if state.mailbox.store_ref().del(&[key.as_bytes()]).is_ok() {
            cleared += 1;
        }
    }
    Json(serde_json::json!({ "cleared": cleared })).into_response()
}

/// `POST /v1/admin/maintenance:bayes-bootstrap` — one-shot seed of the
/// Bayesian spam corpus from the existing Junk (spam) + Inbox (ham)
/// folders (RFC 20260713 §5). Refuses with 409 if the corpus is
/// already populated (a repeat run would double-count). Single-user:
/// the sweep runs for every account.
pub(crate) async fn bayes_bootstrap_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    // Single corpus-empty guard for the whole run — a per-account guard
    // (v2.8.0) let the first trained account lock out every later one.
    if crate::bayes_train::corpus_populated(&state) {
        return (
            axum::http::StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "corpus already populated" })),
        )
            .into_response();
    }
    let mut total_spam = 0u64;
    let mut total_ham = 0u64;
    for user in &users {
        let (s, h) = crate::bayes_train::bootstrap(&state, user);
        total_spam += s;
        total_ham += h;
    }
    Json(serde_json::json!({
        "spam_trained": total_spam,
        "ham_trained": total_ham,
        // Zero and zero is the shape this route returned while its rosters
        // came from zsets nothing writes — it collected nothing and answered
        // 200. `accounts` says whether there was anything to walk at all.
        "accounts": users.len(),
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-triage` — one-shot seed of the
/// v2.9 multi-class triage corpus + retroactive re-sort of existing
/// Inbox mail into Notifications / Promotions. Header-heuristic labels
/// each Inbox thread, re-files N/P out of Inbox, and trains all three
/// classes (so one-vs-rest has data for each). Idempotent. Runs for
/// every account.
pub(crate) async fn backfill_triage_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let (mut inbox, mut notif, mut promo) = (0u64, 0u64, 0u64);
    for user in &users {
        let (i, n, p) = crate::bayes_train::backfill_triage(&state, user);
        inbox += i;
        notif += n;
        promo += p;
    }
    Json(serde_json::json!({
        // What it filed.
        "inbox": inbox,
        "notification": notif,
        "promotion": promo,
        // What it looked at. Three zeros above with `threads_seen` also zero
        // is a blind enumeration, not a mailbox that needed no triage — the
        // distinction this route could not make until 2026-07-31, while it
        // was reading a zset nothing writes.
        "accounts": users.len(),
        "threads_seen": inbox + notif + promo,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-thread-importance` — score
/// threads that predate the feature. `?all=1` rescores every thread;
/// the default only fills in threads with no verdict yet.
pub(crate) async fn backfill_thread_importance_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let only_missing = q.get("all").map(String::as_str) != Some("1");
    let (scored, skipped) = crate::importance::backfill_thread_importance(&state, only_missing);
    tracing::info!(
        scored,
        skipped,
        only_missing,
        "thread importance backfill done"
    );
    axum::Json(serde_json::json!({
        "scored": scored,
        "skipped": skipped,
        // Every enumerated thread is one or the other, so this is the input
        // size: zero here means the walk saw nothing, which is a different
        // fault from a mailbox that is already fully scored.
        "threads_enumerated": scored + skipped,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:backfill-contact-relationships` —
/// one-shot rebuild of the per-sender received/sent counters that
/// importance scoring reads. Runs in process: a `docker exec` helper
/// would open a second embedded kevy and replay the AOF alongside the
/// live one (the 2026-07-13 backfill OOM).
pub(crate) async fn backfill_contact_relationships_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let (users, addresses, messages) = crate::importance::backfill_relationships(&state);
    tracing::info!(users, addresses, messages, "relationship backfill complete");
    axum::Json(serde_json::json!({
        "users": users,
        "addresses": addresses,
        "messages_scanned": messages,
    }))
    .into_response()
}

/// `POST /v1/admin/maintenance:migrate-agent-webhooks` — one namespace.
///
/// The settings page wrote `agent:webhooks:{user}` and the admin surface
/// wrote `admin:webhooks:{account}`, so a subscription created in one was
/// invisible to the other. Both now read the latter; this moves what is left
/// behind, preserving each row's signing secret so existing subscribers keep
/// verifying.
pub(crate) async fn migrate_agent_webhooks_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let addresses = match state.mailbox.list_account_addresses() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match mailrs_core_sidestate::families::webhooks::migrate_agent_namespace(&mut conn, &addresses)
    {
        Ok((moved, accounts)) => Json(serde_json::json!({
            "rows_moved": moved,
            "accounts_examined": accounts,
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "agent webhook migration failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:backfill-webhook-owners` — record who owns
/// each existing webhook subscription.
///
/// Deleting a subscription used to enumerate `mailrs:accounts:index`, a set
/// v2.6.2 stopped writing and the legacy sweep has since deleted, so the
/// loop ran zero times and answered 204: every admin webhook delete has
/// reported success without removing anything. Delete is now an exact
/// lookup through `admin:webhooks:owner`, which only rows created after that
/// change have an entry in. This fills it for the rest.
///
/// fastcore is the only process that can: the accounts live in its embedded
/// kevy and the subscriptions in the shared network one.
pub(crate) async fn backfill_webhook_owners_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let addresses = match state.mailbox.list_account_addresses() {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match mailrs_core_sidestate::families::webhooks::backfill_owner_index(&mut conn, &addresses) {
        Ok((added, accounts)) => Json(serde_json::json!({
            "owners_added": added,
            // Both, so a zero above is legible: no accounts is a different
            // fact from every row already having an owner.
            "accounts_examined": accounts,
        }))
        .into_response(),
        Err(e) => {
            tracing::error!(err = %e, "webhook owner backfill failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `POST /v1/admin/maintenance:sweep-legacy-admin-keys` — one-shot
/// in-process cleanup of the pre-P6 admin keyspace (roadmap Phase
/// 11.2's embedded half, executed as an RPC per
/// `feedback-junk-backfill-oom-finding`: a `docker exec` sweep binary
/// would double-open the embedded kevy and OOM replaying the AOF;
/// running inside the live fastcore process costs nothing).
///
/// Deletes:
///   - `mailrs:alias:<addr>` legacy strings (NOT `mailrs:alias:v2:*`)
///   - `mailrs:domain:<name>` legacy strings (NOT `mailrs:domain:v2:*`)
///   - `mailrs:aliases:index` / `mailrs:domains:index` /
///     `mailrs:accounts:index` legacy sets
///
/// Idempotent — a second call finds nothing and returns zeros. No
/// reader has touched these keys since v2.6.2 (Phase 11.3 removed the
/// last code references); they only weigh down the AOF.
pub(crate) async fn sweep_legacy_admin_keys_route(
    State(state): State<Arc<FastcoreState>>,
) -> axum::response::Response {
    let store = state.mailbox.store_ref();
    let mut aliases = 0u32;
    let mut domains = 0u32;

    let (_, alias_keys) = store.scan(0, Some(b"mailrs:alias:*"), usize::MAX);
    let alias_keys_scanned = alias_keys.len() as u64;
    for key in alias_keys {
        if key.starts_with(b"mailrs:alias:v2:") {
            continue;
        }
        if store.del(&[key.as_slice()]).unwrap_or(0) > 0 {
            aliases += 1;
        }
    }

    let (_, domain_keys) = store.scan(0, Some(b"mailrs:domain:*"), usize::MAX);
    let domain_keys_scanned = domain_keys.len() as u64;
    for key in domain_keys {
        if key.starts_with(b"mailrs:domain:v2:") {
            continue;
        }
        if store.del(&[key.as_slice()]).unwrap_or(0) > 0 {
            domains += 1;
        }
    }

    let indexes = store
        .del(&[
            b"mailrs:aliases:index".as_slice(),
            b"mailrs:domains:index".as_slice(),
            b"mailrs:accounts:index".as_slice(),
        ])
        .unwrap_or(0);

    tracing::info!(
        aliases,
        domains,
        indexes,
        "legacy admin keyspace sweep complete"
    );
    Json(serde_json::json!({
        // What it removed.
        "legacy_alias_strings": aliases,
        "legacy_domain_strings": domains,
        "legacy_index_sets": indexes,
        // What it scanned. Zeros above with these also zero means the scan
        // matched nothing — a different fact from "no legacy keys remain".
        "alias_keys_scanned": alias_keys_scanned,
        "domain_keys_scanned": domain_keys_scanned,
    }))
    .into_response()
}
