//! mailrs-core-api HTTP RPC server, hosted inside the monolith.
//!
//! Phase 2 — checklist `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §2.
//!
//! This module is compiled ONLY when the `core-rpc` cargo feature is on.
//! Without that feature the file is `#[cfg]`-skipped and the monolith
//! produces a byte-identical artifact (ironrule §16 守护点).
//!
//! What this module does:
//!
//! 1. Wraps `Arc<PgMailboxStore>` + `Arc<DomainStore>` + the rest of
//!    server's state into a `Handler` impl from `mailrs_core_api::server`.
//! 2. Mounts the per-method routes onto an `axum::Router`.
//! 3. Binds on `MAILRS_CORE_RPC_ADDR` (defaults to `0.0.0.0:3300`).
//! 4. Verifies `Authorization: Bearer <MAILRS_CORE_API_SECRET>` on
//!    authenticated routes.
//!
//! What this module does NOT do (yet):
//!
//! - Per-method handler bodies — for now only `/v1/healthz` + `/v1/readyz`
//!   are implemented. Per-method routes fill in over subsequent loops
//!   (checklist 2.2).
//! - Touch any existing pg/spg code path; all reads/writes still go via
//!   the existing `Arc<PgMailboxStore>` methods.

#![cfg(feature = "core-rpc")]

mod handlers;

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};

use mailrs_core_api::server::Handler;
use mailrs_core_api::types::{BackendKind, HealthResponse};

/// Aggregate of all state the core RPC server needs to answer requests.
///
/// Cement passes one of these in to `spawn_core_rpc`. The shape mirrors
/// the future `mailrs-core` binary's state struct.
///
/// Fields are `#[allow(dead_code)]` because checklist 2.1 only mounts
/// healthz/readyz; per-method handlers (2.2) will read them.
#[allow(dead_code)]
pub struct CoreRpcState {
    pub mailbox: Arc<mailrs_mailbox::PgMailboxStore>,
    pub domain: Arc<crate::domain_store::DomainStore>,
    /// Direct pool handle — used by `api_key_store` free functions and
    /// other admin paths that don't go through DomainStore.
    pub pool: crate::pg::BackendPool,
}

impl Handler for CoreRpcState {
    async fn healthz(&self) -> HealthResponse {
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            // Today's monolith is the "core" backend (PG/SPG-backed).
            // `fastcore` would set this to `Kevy`.
            backend: BackendKind::Pg,
            // healthz is a liveness probe — process is up.
            ready: true,
        }
    }

    async fn readyz(&self) -> HealthResponse {
        // Phase 2.6 deep probe: acquire a backend pool connection with a
        // tight 1s timeout + run `SELECT 1`. Failure (timeout / acquire
        // error) flips `ready` to false so the orchestrator routes traffic
        // away. This matches the semantics expected by /api/readiness in
        // the existing webapi.
        let ready = match tokio::time::timeout(
            std::time::Duration::from_millis(1000),
            sqlx::query("SELECT 1").execute(&self.pool),
        )
        .await
        {
            Ok(Ok(_)) => true,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "readyz: backend ping returned error");
                false
            }
            Err(_) => {
                tracing::debug!("readyz: backend ping timed out (>1s)");
                false
            }
        };
        HealthResponse {
            version: mailrs_core_api::API_VERSION.into(),
            backend: BackendKind::Pg,
            ready,
        }
    }
}

/// Spawn the `mailrs-core-api` server on the bind address from env.
///
/// `MAILRS_CORE_RPC_ADDR` (default `0.0.0.0:3300`) — listen address.
/// `MAILRS_CORE_API_SECRET` (default empty) — shared bearer secret for
/// authenticated routes. Empty secret means **no auth**, intended for
/// local dev only — production deploys MUST set it.
///
/// Returns immediately; the server runs in a background tokio task.
pub fn spawn_core_rpc(state: Arc<CoreRpcState>, shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    let addr = std::env::var("MAILRS_CORE_RPC_ADDR")
        .unwrap_or_else(|_| format!("0.0.0.0:{}", mailrs_core_api::DEFAULT_CORE_RPC_PORT));

    let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV).unwrap_or_default();
    if secret.is_empty() {
        tracing::warn!(
            event = "core_rpc_no_auth",
            "MAILRS_CORE_API_SECRET unset — core RPC will accept unauthenticated requests"
        );
    }

    let router = build_full_router(state, secret);

    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(error = %e, addr = %addr, "core RPC bind failed");
                return;
            }
        };
        tracing::info!(
            event = "subsystem_started",
            subsystem = "core_rpc",
            addr = %addr,
            "mailrs-core-api server listening"
        );
        let mut shutdown_rx = shutdown_rx;
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            // Drop on `true` — same pattern as the rest of the server.
            let _ = shutdown_rx.changed().await;
        });
        if let Err(e) = server.await {
            tracing::error!(error = %e, "core RPC server exited with error");
        }
    });
}

/// Build the full router with all per-method routes mounted (checklist 2.2)
/// + bearer auth middleware on the authenticated subtree (checklist 2.5).
///
/// Healthz/readyz remain unauthenticated (LB/orchestrator probes).
/// Empty `secret` disables auth entirely — dev-only mode.
fn build_full_router(state: Arc<CoreRpcState>, secret: String) -> Router {
    use mailrs_core_api::method::admin as adm_paths;
    use mailrs_core_api::method::analysis as analysis_paths;
    use mailrs_core_api::method::contact as contact_paths;
    use mailrs_core_api::method::conversation as conv_paths;
    use mailrs_core_api::method::mailbox as mb_paths;
    use mailrs_core_api::method::message as msg_paths;
    use mailrs_core_api::method::thread as th_paths;

    let base = mailrs_core_api::server::base_router(state.clone());

    // ── conversations (Rock 1 + categories + counts) ─────────────────
    let convo = Router::new()
        .route(
            conv_paths::PATH_LIST_CONVERSATIONS,
            post(handlers::conversation::list_conversations),
        )
        .route(
            conv_paths::PATH_CONVERSATIONS_BY_THREAD_IDS,
            post(handlers::conversation::conversations_by_thread_ids),
        )
        .route(
            conv_paths::PATH_CONVERSATION_CATEGORIES,
            get(handlers::conversation::conversation_categories),
        )
        .route(
            conv_paths::PATH_ACTION_COUNT,
            get(handlers::conversation::action_count),
        )
        .route(
            conv_paths::PATH_UNSEEN_COUNT,
            get(handlers::conversation::unseen_count),
        )
        .with_state(state.clone());

    // ── mailbox CRUD ────────────────────────────────────────────────
    let mb = Router::new()
        .route(
            mb_paths::PATH_LIST_MAILBOXES,
            get(handlers::mailbox::list_mailboxes),
        )
        .route(
            mb_paths::PATH_GET_MAILBOX,
            get(handlers::mailbox::get_mailbox),
        )
        .route(
            mb_paths::PATH_GET_MAILBOX_BY_ID,
            get(handlers::mailbox::get_mailbox_by_id),
        )
        .route(
            mb_paths::PATH_CREATE_MAILBOX,
            post(handlers::mailbox::create_mailbox),
        )
        .route(
            mb_paths::PATH_DELETE_MAILBOX,
            delete(handlers::mailbox::delete_mailbox),
        )
        .route(
            mb_paths::PATH_RENAME_MAILBOX,
            post(handlers::mailbox::rename_mailbox),
        )
        .route(
            mb_paths::PATH_MAILBOX_STATUS,
            get(handlers::mailbox::mailbox_status),
        )
        .with_state(state.clone());

    // ── thread mutate ────────────────────────────────────────────────
    let th = Router::new()
        .route(th_paths::PATH_MARK_READ, post(handlers::thread::mark_read))
        .route(
            th_paths::PATH_MARK_UNREAD,
            post(handlers::thread::mark_unread),
        )
        .route(th_paths::PATH_STAR, post(handlers::thread::star))
        .route(th_paths::PATH_UNSTAR, post(handlers::thread::unstar))
        .route(th_paths::PATH_PIN, post(handlers::thread::pin))
        .route(th_paths::PATH_UNPIN, post(handlers::thread::unpin))
        .route(th_paths::PATH_ARCHIVE, post(handlers::thread::archive))
        .route(th_paths::PATH_UNARCHIVE, post(handlers::thread::unarchive))
        .route(
            th_paths::PATH_DISMISS_ACTION,
            post(handlers::thread::dismiss_action),
        )
        .route(th_paths::PATH_SNOOZE, put(handlers::thread::snooze))
        .route(th_paths::PATH_UNSNOOZE, delete(handlers::thread::unsnooze))
        .route(
            th_paths::PATH_DELETE_THREAD,
            delete(handlers::thread::delete_thread),
        )
        .with_state(state.clone());

    // ── message read ─────────────────────────────────────────────────
    let msg = Router::new()
        .route(
            msg_paths::PATH_GET_MESSAGE_BY_UID,
            get(handlers::message::get_message_by_uid),
        )
        .route(
            msg_paths::PATH_LIST_MESSAGES,
            get(handlers::message::list_messages),
        )
        .route(
            msg_paths::PATH_FIND_BY_MESSAGE_ID,
            get(handlers::message::find_message_by_message_id),
        )
        .route(
            msg_paths::PATH_SET_FLAGS,
            put(handlers::message::flag_mutation),
        )
        .route(
            msg_paths::PATH_FLAGS_IF_UNCHANGED,
            post(handlers::message::condstore),
        )
        .route(
            msg_paths::PATH_CHANGED_SINCE,
            get(handlers::message::changed_since),
        )
        .route(msg_paths::PATH_EXPUNGE, post(handlers::message::expunge))
        .route(
            msg_paths::PATH_COPY_MESSAGE,
            post(handlers::message::copy_message),
        )
        .route(
            msg_paths::PATH_MOVE_MESSAGE,
            post(handlers::message::move_message),
        )
        .with_state(state.clone());

    // ── admin (auth hot path) ─────────────────────────────────────────
    let adm = Router::new()
        .route(
            adm_paths::PATH_GET_API_KEY_BY_PREFIX,
            get(handlers::admin::get_api_key_by_prefix),
        )
        .route(
            adm_paths::PATH_TOUCH_API_KEY,
            post(handlers::admin::touch_api_key),
        )
        .route(
            adm_paths::PATH_EFFECTIVE_PERMISSIONS,
            get(handlers::admin::effective_permissions),
        )
        .route(
            adm_paths::PATH_GET_ACCOUNT_HASH,
            get(handlers::admin::get_account_with_hash),
        )
        .route(
            adm_paths::PATH_LIST_ACCOUNTS,
            get(handlers::admin::list_accounts),
        )
        .route(
            adm_paths::PATH_GET_ACCOUNT,
            get(handlers::admin::get_account),
        )
        .with_state(state.clone());

    // ── analysis ─────────────────────────────────────────────────────
    let anal = Router::new()
        .route(
            analysis_paths::PATH_GET_ANALYSIS,
            get(handlers::analysis::get_analysis),
        )
        .route(
            analysis_paths::PATH_COUNT_UNANALYZED,
            get(handlers::analysis::count_unanalyzed),
        )
        .route(
            analysis_paths::PATH_BOOST_IMPORTANCE,
            post(handlers::analysis::boost_importance),
        )
        .route(
            analysis_paths::PATH_ATTACHMENT_TEXTS,
            get(handlers::analysis::attachment_texts),
        )
        .route(
            analysis_paths::PATH_SEMANTIC_SEARCH,
            post(handlers::analysis::semantic_search),
        )
        .with_state(state.clone());

    // ── contacts ─────────────────────────────────────────────────────
    let ct = Router::new()
        .route(
            contact_paths::PATH_SEARCH_CONTACTS,
            get(handlers::contact::search_contacts),
        )
        .route(
            contact_paths::PATH_UPSERT_INBOUND,
            post(handlers::contact::upsert_inbound),
        )
        .route(
            contact_paths::PATH_CONTACT_SCORING,
            get(handlers::contact::contact_scoring),
        )
        .route(
            contact_paths::PATH_HAS_SENT_TO,
            get(handlers::contact::has_sent_to),
        )
        .with_state(state.clone());

    // Authenticated subtree = everything except /v1/healthz + /v1/readyz.
    let authenticated = convo
        .merge(mb)
        .merge(th)
        .merge(msg)
        .merge(adm)
        .merge(anal)
        .merge(ct);
    drop(state);

    // Auth middleware applies only when a secret was configured. Empty
    // secret = dev/local mode, no auth.
    if secret.is_empty() {
        base.merge(authenticated)
    } else {
        let expected = Arc::new(secret);
        let authenticated = authenticated.layer(axum::middleware::from_fn_with_state(
            expected,
            mailrs_core_api::server::auth_middleware,
        ));
        base.merge(authenticated)
    }
}
