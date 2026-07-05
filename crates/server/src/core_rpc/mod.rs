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
    /// Maildir root — handlers that serve raw bytes look up the file
    /// under `{maildir_root}/{user}/cur|new/{maildir_id}`.
    pub maildir_root: String,
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
    use mailrs_core_api::method::outbound as ob_paths;
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
        // GET lists, POST ingests — same URL as PATH_DELIVER_MESSAGE, so
        // the two share one method-chained route (separate .route() calls
        // on an identical path panic at startup).
        .route(
            th_paths::PATH_LIST_THREAD_MESSAGES,
            get(handlers::thread::list_thread_messages).post(handlers::thread::deliver_message),
        )
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
            msg_paths::PATH_GET_MESSAGE_BY_UID_USER,
            get(handlers::message::get_message_by_uid_for_user),
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
            "/v1/mailboxes/{id}/messages/uid/{uid}/raw",
            get(handlers::message::get_message_raw),
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
            get(handlers::admin::list_accounts).post(handlers::admin::add_account),
        )
        // GET_ACCOUNT / UPDATE_ACCOUNT / REMOVE_ACCOUNT share one URL
        .route(
            adm_paths::PATH_GET_ACCOUNT,
            get(handlers::admin::get_account)
                .put(handlers::admin::update_account)
                .delete(handlers::admin::remove_account),
        )
        .route(adm_paths::PATH_SET_QUOTA, post(handlers::admin::set_quota))
        .route(
            adm_paths::PATH_UPDATE_RECOVERY_EMAIL,
            post(handlers::admin::set_recovery_email),
        )
        .route(
            adm_paths::PATH_SET_ACCOUNT_PASSWORD,
            post(handlers::admin::set_account_password),
        )
        .route(
            adm_paths::PATH_SET_MESSAGE_FLAGS,
            post(handlers::admin::set_message_flags),
        )
        // aliases (id-based, legacy) + source-keyed (v2 backend-neutral)
        .route(
            adm_paths::PATH_LIST_ALIASES,
            get(handlers::admin::list_aliases).post(handlers::admin::add_alias),
        )
        .route(
            adm_paths::PATH_REMOVE_ALIAS,
            delete(handlers::admin::remove_alias),
        )
        .route(
            "/v1/admin/aliases:local",
            get(handlers::admin::list_local_aliases).post(handlers::admin::upsert_local_alias),
        )
        .route(
            "/v1/admin/aliases:local/{source}",
            delete(handlers::admin::delete_local_alias),
        )
        // domains
        .route(
            adm_paths::PATH_LIST_DOMAINS,
            get(handlers::admin::list_domains).post(handlers::admin::add_domain),
        )
        .route(
            adm_paths::PATH_REMOVE_DOMAIN,
            delete(handlers::admin::remove_domain),
        )
        // sieve
        .route(
            adm_paths::PATH_GET_SIEVE,
            get(handlers::admin::get_sieve)
                .post(handlers::admin::set_sieve)
                .delete(handlers::admin::delete_sieve),
        )
        // audit log
        .route(
            adm_paths::PATH_LIST_AUDIT_LOG,
            get(handlers::admin::list_audit_log).post(handlers::admin::log_audit),
        )
        // groups + permissions
        .route(
            adm_paths::PATH_LIST_GROUPS,
            get(handlers::admin::list_groups),
        )
        .route(
            adm_paths::PATH_GET_GROUP_PERMISSIONS,
            get(handlers::admin::get_group_permissions).put(handlers::admin::set_group_permissions),
        )
        .route(
            adm_paths::PATH_LIST_GROUP_MEMBERS,
            get(handlers::admin::list_group_members).post(handlers::admin::add_account_to_group),
        )
        .route(
            adm_paths::PATH_REMOVE_ACCOUNT_FROM_GROUP,
            delete(handlers::admin::remove_account_from_group),
        )
        .route(
            adm_paths::PATH_GET_ACCOUNT_GROUPS,
            get(handlers::admin::get_account_groups),
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
        .route(
            contact_paths::PATH_SENDER_FEEDBACK,
            post(handlers::contact::sender_feedback),
        )
        .with_state(state.clone());

    // ── drafts ──────────────────────────────────────────────────────
    let drafts = Router::new()
        .route(
            adm_paths::PATH_LIST_DRAFTS,
            get(handlers::drafts::list_drafts).post(handlers::drafts::save_draft),
        )
        .route(
            adm_paths::PATH_DELETE_DRAFT,
            delete(handlers::drafts::delete_draft),
        )
        .with_state(state.clone());

    // ── signatures ──────────────────────────────────────────────────
    let signatures = Router::new()
        .route(
            adm_paths::PATH_LIST_SIGNATURES,
            get(handlers::signatures::list_signatures).post(handlers::signatures::save_signature),
        )
        .route(
            adm_paths::PATH_DELETE_SIGNATURE,
            delete(handlers::signatures::delete_signature),
        )
        .with_state(state.clone());

    // ── webhooks ─────────────────────────────────────────────────────
    let webhooks = Router::new()
        .route(
            adm_paths::PATH_CREATE_WEBHOOK,
            post(handlers::webhooks::create_webhook),
        )
        .route(
            adm_paths::PATH_LIST_WEBHOOKS,
            get(handlers::webhooks::list_webhooks),
        )
        .route(
            adm_paths::PATH_DELETE_WEBHOOK,
            delete(handlers::webhooks::delete_webhook),
        )
        .with_state(state.clone());

    // ── templates ────────────────────────────────────────────────────
    let templates = Router::new()
        .route(
            adm_paths::PATH_LIST_TEMPLATES,
            get(handlers::templates::list_templates).post(handlers::templates::save_template),
        )
        .route(
            adm_paths::PATH_DELETE_TEMPLATE,
            delete(handlers::templates::delete_template),
        )
        .with_state(state.clone());

    // ── reactions ────────────────────────────────────────────────────
    let rx = Router::new()
        .route(
            adm_paths::PATH_GET_THREAD_REACTIONS,
            get(handlers::reactions::get_thread_reactions),
        )
        .route(
            adm_paths::PATH_TOGGLE_REACTION,
            put(handlers::reactions::toggle_reaction),
        )
        .with_state(state.clone());

    // ── outbound (sender ↔ core) ─────────────────────────────────────
    let ob = Router::new()
        .route(ob_paths::PATH_ENQUEUE, post(handlers::outbound::enqueue))
        .route(ob_paths::PATH_CLAIM, post(handlers::outbound::claim))
        .route(ob_paths::PATH_STATS, get(handlers::outbound::stats))
        .route(
            ob_paths::PATH_RECOVER_STALE,
            post(handlers::outbound::recover_stale),
        )
        .route(
            ob_paths::PATH_MARK_DELIVERED,
            post(handlers::outbound::mark_delivered),
        )
        .route(
            ob_paths::PATH_MARK_FAILED,
            post(handlers::outbound::mark_failed),
        )
        .route(
            ob_paths::PATH_MARK_BOUNCED,
            post(handlers::outbound::mark_bounced),
        )
        .with_state(state.clone());

    // Authenticated subtree = everything except /v1/healthz + /v1/readyz.
    let authenticated = convo
        .merge(mb)
        .merge(th)
        .merge(msg)
        .merge(adm)
        .merge(anal)
        .merge(ct)
        .merge(drafts)
        .merge(signatures)
        .merge(templates)
        .merge(webhooks)
        .merge(rx)
        .merge(ob);
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

/// Route-surface lock (v2 point 3): the pg-core and fastcore MUST serve
/// the identical core-api contract. This asserts every PATH_* the pg-core
/// mounts is also mounted by fastcore, by parsing both `build_*_router`
/// source bodies. Any future route added to one core but not the other
/// fails this test — the cores can never silently diverge again.
#[cfg(test)]
mod route_parity_lock {
    /// Extract the `PATH_*` identifiers referenced inside the first
    /// `fn <name>(` … balanced-brace body in `src`.
    fn router_paths(src: &str, fn_marker: &str) -> std::collections::BTreeSet<String> {
        let start = src.find(fn_marker).expect("router fn present");
        let body = &src[start..];
        // take until the function's closing — good enough: scan to the next
        // top-level "\n}\n" after the fn (routers end with `.with_state`/merge).
        let end = body.find("\n}\n").map(|e| e + start).unwrap_or(src.len());
        let region = &src[start..end];
        let mut out = std::collections::BTreeSet::new();
        let bytes = region.as_bytes();
        let mut i = 0;
        while let Some(pos) = region[i..].find("PATH_") {
            let s = i + pos;
            let mut e = s;
            while e < bytes.len() && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'_') {
                e += 1;
            }
            out.insert(region[s..e].to_string());
            i = e;
        }
        out
    }

    #[test]
    fn fastcore_serves_every_pg_core_route() {
        let pg_src = include_str!("mod.rs");
        let fc_src = include_str!("../../../fastcore/src/lib.rs");
        let pg = router_paths(pg_src, "fn build_full_router");
        let fc = router_paths(fc_src, "fn build_router");
        let missing: Vec<_> = pg.difference(&fc).cloned().collect();
        assert!(
            missing.is_empty(),
            "fastcore is missing pg-core contract routes (the two cores diverged): {missing:?}"
        );
    }
}

#[cfg(all(test, feature = "spg"))]
mod pg_core_tests {
    //! In-process validation that the PG core router mounts + serves the
    //! v2 dual-mode routes (deliver_message ingest + read-back +
    //! Message-ID idempotency) against a real in-memory spg store.
    //! Spawns the router on an ephemeral port and drives it with the
    //! core-api Client — the same surface `mailrs-core-sync` uses.

    use super::*;
    use mailrs_core_api::client::Client;
    use mailrs_core_api::method::admin::AddAccountRequest;
    use mailrs_core_api::method::thread::DeliverMessageRequest;
    use spg_sqlx::SpgPoolExt;

    const SCHEMA_SQL: &str = include_str!("../../../../scripts/init-schema.sql");

    async fn spawn_pg_core() -> String {
        let pool = spg_sqlx::SpgPool::connect_in_memory()
            .await
            .expect("open in-memory spg");
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .expect("apply init-schema.sql");
        // domain FK for account inserts
        sqlx::query("INSERT INTO domains (name) VALUES ('test') ON CONFLICT DO NOTHING")
            .execute(&pool)
            .await
            .unwrap();

        let mailbox = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone()));
        let domain = Arc::new(crate::domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
            crate::health::HealthState::new(),
        ));
        let state = Arc::new(CoreRpcState {
            mailbox,
            domain,
            pool,
            maildir_root: "/tmp/pg-core-test".into(),
        });
        let router = build_full_router(state, String::new());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn deliver_req(
        message_id: &str,
        uid: u32,
        thread_id: &str,
        user: &str,
    ) -> DeliverMessageRequest {
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": uid,
            "blob_ref": format!("{message_id}.host"),
            "sender": "remote@x.y", "recipients": user, "subject": "Hi",
            "date": 1_700_000_000i64, "internal_date": 1_700_000_000i64,
            "size": 42, "flags": 1, "message_id": message_id,
            "in_reply_to": "", "thread_id": thread_id, "modseq": 0,
            "user_address": user,
        });
        DeliverMessageRequest {
            message_id: message_id.into(),
            subject: "Hi".into(),
            senders_csv: "remote@x.y".into(),
            latest_date: 1_700_000_000,
            latest_preview: String::new(),
            category: "inbox".into(),
            unread: true,
            uid,
            payload_wire_json: wire.to_string(),
        }
    }

    #[tokio::test]
    async fn pg_core_ingest_reads_back_and_is_idempotent() {
        let base = spawn_pg_core().await;
        let c = Client::new(base, String::new());
        let user = "u@test";
        let thread = "t1@test";

        c.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "U".into(),
            password: "pw".into(),
        })
        .await
        .expect("add_account");

        // ingest a message via the P0 route
        c.deliver_message(user, thread, &deliver_req("m1@test", 1, thread, user))
            .await
            .expect("deliver_message");

        // read it back over the contract
        let msgs = c.list_thread_messages(user, thread).await.expect("list");
        assert_eq!(msgs.items.len(), 1, "ingested message must read back");
        assert_eq!(msgs.items[0].message_id, "m1@test");

        // Message-ID idempotent: re-deliver is a no-op, still one message
        c.deliver_message(user, thread, &deliver_req("m1@test", 1, thread, user))
            .await
            .expect("re-deliver");
        let msgs2 = c.list_thread_messages(user, thread).await.expect("list2");
        assert_eq!(msgs2.items.len(), 1, "re-delivery must not duplicate");
    }

    // ── real cross-backend sync (v2 audit point 5) ──────────────────
    // Spawn a live kevy fastcore AND the spg pg-core, then run the real
    // mailrs-core-sync between them over HTTP. This is the actual
    // production switch axis (kevy↔pg), which the kevy↔kevy roundtrip
    // test could not cover.

    fn spawn_fastcore() -> String {
        use std::sync::Arc as StdArc;
        let store =
            StdArc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
        let state = StdArc::new(mailrs_fastcore::FastcoreState::new(
            mailrs_mailbox_kevy::KevyMailboxStore::new(store),
        ));
        let router = mailrs_fastcore::build_router(state);
        // bind synchronously so the caller has the URL before returning
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn sync_kevy_to_pg_mirrors_mail_store() {
        use mailrs_core_sync::{SyncOpts, sync};

        let kevy_base = spawn_fastcore();
        let pg_base = spawn_pg_core().await;
        let kevy = Client::new(kevy_base, String::new());
        let pg = Client::new(pg_base, String::new());
        let user = "sync-user@test";

        // seed the kevy core with an account + 2 threads x 2 messages
        kevy.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "Sync".into(),
            password: "pw".into(),
        })
        .await
        .expect("seed add_account");
        let mut uid = 1u32;
        for t in 0..2 {
            let thread = format!("th-{t}@test");
            for m in 0..2 {
                kevy.deliver_message(
                    user,
                    &thread,
                    &deliver_req(&format!("k-{t}-{m}@test"), uid, &thread, user),
                )
                .await
                .expect("seed deliver");
                uid += 1;
            }
        }

        // run the REAL sync kevy -> pg over the contract
        let report = sync(&kevy, &pg, &SyncOpts::default())
            .await
            .expect("kevy->pg sync");
        assert_eq!(report.accounts, 1);
        assert_eq!(report.messages_delivered, 4, "all 4 messages cross to pg");
        assert_eq!(report.messages_skipped_dupe, 0);

        // assert the pg core now holds the same per-thread message-id set
        for t in 0..2 {
            let thread = format!("th-{t}@test");
            let msgs = pg
                .list_thread_messages(user, &thread)
                .await
                .expect("pg list");
            let ids: std::collections::BTreeSet<String> =
                msgs.items.iter().map(|m| m.message_id.clone()).collect();
            let expected: std::collections::BTreeSet<String> =
                (0..2).map(|m| format!("k-{t}-{m}@test")).collect();
            assert_eq!(ids, expected, "pg thread {t} mirrors kevy");
        }

        // re-run: idempotent (per-thread dedup on the pg side)
        let report2 = sync(&kevy, &pg, &SyncOpts::default())
            .await
            .expect("re-sync");
        assert_eq!(report2.messages_delivered, 0, "re-run delivers nothing new");
        assert_eq!(
            report2.messages_skipped_dupe, 4,
            "re-run skips all 4 as dupes"
        );
    }

    #[tokio::test]
    async fn sync_pg_to_kevy_mirrors_mail_store() {
        // reverse direction: seed the PG core, sync -> kevy. Exercises the
        // pg-core's enumeration READ path (list_conversations /
        // list_thread_messages against PG) + kevy ingest, proving the
        // switch is reliable in BOTH directions across the real boundary.
        use mailrs_core_sync::{SyncOpts, sync};

        let pg_base = spawn_pg_core().await;
        let kevy_base = spawn_fastcore();
        let pg = Client::new(pg_base, String::new());
        let kevy = Client::new(kevy_base, String::new());
        let user = "rev-user@test";

        pg.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "Rev".into(),
            password: "pw".into(),
        })
        .await
        .expect("seed add_account");
        let mut uid = 1u32;
        for t in 0..2 {
            let thread = format!("rth-{t}@test");
            for m in 0..2 {
                pg.deliver_message(
                    user,
                    &thread,
                    &deliver_req(&format!("p-{t}-{m}@test"), uid, &thread, user),
                )
                .await
                .expect("seed deliver into pg");
                uid += 1;
            }
        }

        let report = sync(&pg, &kevy, &SyncOpts::default())
            .await
            .expect("pg->kevy sync");
        assert_eq!(report.accounts, 1);
        assert_eq!(report.messages_delivered, 4, "all 4 messages cross to kevy");

        for t in 0..2 {
            let thread = format!("rth-{t}@test");
            let msgs = kevy
                .list_thread_messages(user, &thread)
                .await
                .expect("kevy list");
            let ids: std::collections::BTreeSet<String> =
                msgs.items.iter().map(|m| m.message_id.clone()).collect();
            let expected: std::collections::BTreeSet<String> =
                (0..2).map(|m| format!("p-{t}-{m}@test")).collect();
            assert_eq!(ids, expected, "kevy thread {t} mirrors pg");
        }
    }

    // ── contract parity on the webapi-called surface (audit point 3) ──
    // fastcore and pg-core are NOT identical across the FULL contract
    // (pg-core serves ~60 routes fastcore 404s). But webapi only calls a
    // mail-store subset, and for THAT subset the two must be
    // substitutable. This asserts semantic equality (normalized, not
    // byte-identical — kevy synthesizes mailbox ids/uidvalidity) on the
    // read surface after an identical seed.

    async fn seed_core(c: &Client, user: &str) {
        c.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "Parity".into(),
            password: "pw".into(),
        })
        .await
        .expect("seed add_account");
        let mut uid = 1u32;
        for t in 0..3 {
            let thread = format!("pth-{t}@test");
            for m in 0..2 {
                c.deliver_message(
                    user,
                    &thread,
                    &deliver_req(&format!("x-{t}-{m}@test"), uid, &thread, user),
                )
                .await
                .expect("seed deliver");
                uid += 1;
            }
        }
    }

    /// Normalized (thread_id -> sorted message-id set) for a user.
    async fn thread_msg_map(c: &Client) -> std::collections::BTreeMap<String, Vec<String>> {
        use mailrs_core_api::method::conversation::ListConversationsRequest;
        use mailrs_core_api::types::ConversationFilter;
        let user = "parity@test";
        let mut out = std::collections::BTreeMap::new();
        let page = c
            .list_conversations(
                user,
                &ListConversationsRequest {
                    filter: ConversationFilter {
                        limit: 100,
                        ..Default::default()
                    },
                },
            )
            .await
            .expect("list_conversations");
        for s in &page.items {
            let msgs = c
                .list_thread_messages(user, &s.thread_id)
                .await
                .expect("list_thread_messages");
            let mut ids: Vec<String> = msgs.items.iter().map(|m| m.message_id.clone()).collect();
            ids.sort();
            out.insert(s.thread_id.clone(), ids);
        }
        out
    }

    #[tokio::test]
    async fn fastcore_and_pgcore_agree_on_webapi_read_surface() {
        let user = "parity@test";
        let kevy = Client::new(spawn_fastcore(), String::new());
        let pg = Client::new(spawn_pg_core().await, String::new());

        // identical seed into both cores
        seed_core(&kevy, user).await;
        seed_core(&pg, user).await;

        // both must expose the identical thread -> message-id structure
        let kmap = thread_msg_map(&kevy).await;
        let pmap = thread_msg_map(&pg).await;
        assert_eq!(
            kmap, pmap,
            "fastcore and pg-core must agree on threads+messages"
        );
        assert_eq!(kmap.len(), 3, "3 threads on both");
        for (_t, ids) in &kmap {
            assert_eq!(ids.len(), 2, "2 messages per thread on both");
        }

        // account listing agrees on the seeded address
        let kaccts: std::collections::BTreeSet<String> = kevy
            .list_accounts()
            .await
            .expect("kevy accounts")
            .items
            .into_iter()
            .map(|a| a.address)
            .collect();
        let paccts: std::collections::BTreeSet<String> = pg
            .list_accounts()
            .await
            .expect("pg accounts")
            .items
            .into_iter()
            .map(|a| a.address)
            .collect();
        assert!(
            kaccts.contains(user) && paccts.contains(user),
            "both list the account"
        );
    }

    // ── SQL-core ↔ SQL-core sync (audit point 6, pg↔spg mechanism) ────
    // pg and spg are the SAME PgMailboxStore code over sqlx — they differ
    // only in the storage engine underneath, which sqlx abstracts. So the
    // pg↔spg switch reuses mailrs-core-sync between two SQL cores. This
    // runs it between two spg cores, proving a SQL-backed core works as
    // BOTH sync source and destination over the contract (the exact
    // read=list_conversations + write=deliver_message mechanism a real
    // pg↔spg migration uses). CAVEAT: real-PostgreSQL↔real-spg with
    // distinct on-disk formats + spg's held bugs remains untested.
    #[tokio::test]
    async fn sync_sql_core_to_sql_core() {
        use mailrs_core_sync::{SyncOpts, sync};

        let src = Client::new(spawn_pg_core().await, String::new());
        let dst = Client::new(spawn_pg_core().await, String::new());
        let user = "sql-sync@test";

        src.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "Sql".into(),
            password: "pw".into(),
        })
        .await
        .expect("seed add_account");
        let mut uid = 1u32;
        for t in 0..2 {
            let thread = format!("sth-{t}@test");
            for m in 0..2 {
                src.deliver_message(
                    user,
                    &thread,
                    &deliver_req(&format!("s-{t}-{m}@test"), uid, &thread, user),
                )
                .await
                .expect("seed deliver");
                uid += 1;
            }
        }

        let report = sync(&src, &dst, &SyncOpts::default())
            .await
            .expect("sql->sql sync");
        assert_eq!(report.messages_delivered, 4, "all 4 cross SQL->SQL");

        for t in 0..2 {
            let thread = format!("sth-{t}@test");
            let msgs = dst
                .list_thread_messages(user, &thread)
                .await
                .expect("dst list");
            let ids: std::collections::BTreeSet<String> =
                msgs.items.iter().map(|m| m.message_id.clone()).collect();
            let expected: std::collections::BTreeSet<String> =
                (0..2).map(|m| format!("s-{t}-{m}@test")).collect();
            assert_eq!(ids, expected, "dst SQL core thread {t} mirrors src");
        }
    }
}

#[cfg(all(test, feature = "core-rpc", not(feature = "spg")))]
mod real_pg_sync_tests {
    //! Cross-backend sync against REAL PostgreSQL (not in-memory spg).
    //! Spins a pgvector:pg18 testcontainer, builds the pg-core over the
    //! real PgPool, spawns a kevy fastcore, and runs the real
    //! mailrs-core-sync BOTH directions — the highest-fidelity proof of
    //! the kevy↔pg production switch axis (audit point 5, real Postgres).
    //!
    //! Requires docker; runs on the default (non-spg) axis with
    //! `--features core-rpc`. Serialized via its own container.

    use super::*;
    use mailrs_core_api::client::Client;
    use mailrs_core_api::method::admin::AddAccountRequest;
    use mailrs_core_api::method::thread::DeliverMessageRequest;
    use mailrs_core_sync::{SyncOpts, sync};
    use testcontainers::{
        ContainerAsync, GenericImage, ImageExt,
        core::{IntoContainerPort, WaitFor},
        runners::AsyncRunner,
    };

    const SCHEMA_SQL: &str = include_str!("../../../../scripts/init-schema.sql");

    async fn start_pg() -> (ContainerAsync<GenericImage>, crate::pg::BackendPool) {
        let container = GenericImage::new("pgvector/pgvector", "pg18")
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ))
            .with_exposed_port(5432.tcp())
            .with_env_var("POSTGRES_PASSWORD", "test")
            .with_env_var("POSTGRES_DB", "mailrs_test")
            .with_env_var("POSTGRES_USER", "postgres")
            .start()
            .await
            .expect("start pgvector");
        let host = container.get_host().await.expect("host");
        let port = container.get_host_port_ipv4(5432).await.expect("port");
        let url = format!("postgres://postgres:test@{host}:{port}/mailrs_test");
        // race the listener readiness
        let pool = {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                match sqlx::PgPool::connect(&url).await {
                    Ok(p) => break p,
                    Err(_) if std::time::Instant::now() < deadline => {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                    Err(e) => panic!("pg never came up: {e}"),
                }
            }
        };
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&pool)
            .await
            .expect("apply schema");
        sqlx::query("INSERT INTO domains (name) VALUES ('test') ON CONFLICT DO NOTHING")
            .execute(&pool)
            .await
            .unwrap();
        (container, pool)
    }

    fn spawn_pg_core(pool: crate::pg::BackendPool) -> String {
        let mailbox = Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone()));
        let domain = Arc::new(crate::domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
            crate::health::HealthState::new(),
        ));
        let state = Arc::new(CoreRpcState {
            mailbox,
            domain,
            pool,
            maildir_root: "/tmp/real-pg-core-test".into(),
        });
        let router = build_full_router(state, String::new());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let l = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(l, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn spawn_fastcore() -> String {
        let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
        let state = Arc::new(mailrs_fastcore::FastcoreState::new(
            mailrs_mailbox_kevy::KevyMailboxStore::new(store),
        ));
        let router = mailrs_fastcore::build_router(state);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let l = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(l, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn deliver_req(mid: &str, uid: u32, thread: &str, user: &str) -> DeliverMessageRequest {
        let wire = serde_json::json!({
            "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": format!("{mid}.host"),
            "sender": "remote@x.y", "recipients": user, "subject": "Hi",
            "date": 1_700_000_000i64, "internal_date": 1_700_000_000i64,
            "size": 42, "flags": 1, "message_id": mid, "in_reply_to": "",
            "thread_id": thread, "modseq": 0, "user_address": user,
        });
        DeliverMessageRequest {
            message_id: mid.into(),
            subject: "Hi".into(),
            senders_csv: "remote@x.y".into(),
            latest_date: 1_700_000_000,
            latest_preview: String::new(),
            category: "inbox".into(),
            unread: true,
            uid,
            payload_wire_json: wire.to_string(),
        }
    }

    async fn thread_ids(
        c: &Client,
        user: &str,
        threads: usize,
    ) -> Vec<std::collections::BTreeSet<String>> {
        let mut out = Vec::new();
        for t in 0..threads {
            let thread = format!("rp-{t}@test");
            let msgs = c.list_thread_messages(user, &thread).await.expect("list");
            out.push(msgs.items.iter().map(|m| m.message_id.clone()).collect());
        }
        out
    }

    #[tokio::test]
    async fn real_pg_bidirectional_sync() {
        let (_container, pool) = start_pg().await;
        let pg = Client::new(spawn_pg_core(pool), String::new());
        let kevy = Client::new(spawn_fastcore(), String::new());
        let user = "realpg@test";

        // seed the kevy core
        kevy.add_account(&AddAccountRequest {
            address: user.into(),
            display_name: "Real".into(),
            password: "pw".into(),
        })
        .await
        .expect("add_account");
        let mut uid = 1u32;
        for t in 0..2 {
            let thread = format!("rp-{t}@test");
            for m in 0..2 {
                kevy.deliver_message(
                    user,
                    &thread,
                    &deliver_req(&format!("rp-{t}-{m}@test"), uid, &thread, user),
                )
                .await
                .expect("seed deliver");
                uid += 1;
            }
        }

        // kevy -> REAL pg
        let r = sync(&kevy, &pg, &SyncOpts::default())
            .await
            .expect("kevy->pg");
        assert_eq!(r.messages_delivered, 4, "4 msgs land in real Postgres");
        let expected: Vec<std::collections::BTreeSet<String>> = (0..2)
            .map(|t| (0..2).map(|m| format!("rp-{t}-{m}@test")).collect())
            .collect();
        assert_eq!(
            thread_ids(&pg, user, 2).await,
            expected,
            "real pg mirrors kevy"
        );

        // re-run idempotent against real pg (find_by_message_id guard)
        let r2 = sync(&kevy, &pg, &SyncOpts::default())
            .await
            .expect("re-sync");
        assert_eq!(r2.messages_delivered, 0, "real-pg re-run delivers nothing");

        // reverse: REAL pg -> a fresh kevy
        let kevy2 = Client::new(spawn_fastcore(), String::new());
        let rr = sync(&pg, &kevy2, &SyncOpts::default())
            .await
            .expect("pg->kevy");
        assert_eq!(rr.messages_delivered, 4, "4 msgs cross back from real pg");
        assert_eq!(
            thread_ids(&kevy2, user, 2).await,
            expected,
            "kevy2 mirrors real pg"
        );
    }
}
