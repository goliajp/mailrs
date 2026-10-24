// CARVE-OUT: pure route registration; the builder chain is the table
//! The core-RPC route table.
//!
//! Split out of `core_rpc/mod.rs` on 2026-08-02. `route_parity_lock` in `tests/parity.rs` is
//! what holds this table to the fastcore lane's — the two cores serve one client, so a path
//! here and not there is a feature that works or 404s depending on which is deployed.

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};

use super::*;

/// Build the full router with all per-method routes mounted (checklist 2.2)
/// + bearer auth middleware on the authenticated subtree (checklist 2.5).
///
/// Healthz/readyz remain unauthenticated (LB/orchestrator probes).
/// Empty `secret` disables auth entirely — dev-only mode.
pub(super) fn build_full_router(state: Arc<CoreRpcState>, secret: String) -> Router {
    use mailrs_core_api::method::admin as adm_paths;
    use mailrs_core_api::method::conversation as conv_paths;
    use mailrs_core_api::method::mailbox as mb_paths;
    use mailrs_core_api::method::message as msg_paths;
    use mailrs_core_api::method::sent as sent_paths;
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
            conv_paths::PATH_UNSEEN_COUNT,
            get(handlers::conversation::unseen_count),
        )
        .with_state(state.clone());

    // ── mailboxes ───────────────────────────────────────────────────
    let mb = Router::new()
        .route(
            mb_paths::PATH_LIST_MAILBOXES,
            get(handlers::mailbox::list_mailboxes),
        )
        .with_state(state.clone());

    // ── thread mutate ────────────────────────────────────────────────
    let th = Router::new()
        // GET lists, POST ingests — same URL as PATH_DELIVER_MESSAGE, so
        // the two share one method-chained route (separate .route() calls
        // on an identical path panic at startup).
        .route(
            th_paths::PATH_LIST_THREAD_MESSAGES,
            get(handlers::thread::list_thread_messages).post(handlers::deliver::deliver_message),
        )
        .route(sent_paths::PATH_FILE_SENT, post(handlers::sent::file_sent))
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
            msg_paths::PATH_GET_MESSAGE_BY_UID_USER,
            get(handlers::message::get_message_by_uid_for_user),
        )
        .route(
            msg_paths::PATH_FIND_BY_MESSAGE_ID,
            get(handlers::message::find_message_by_message_id),
        )
        .with_state(state.clone());

    // ── admin (auth hot path) ─────────────────────────────────────────
    let adm = Router::new()
        .route(
            adm_paths::PATH_EFFECTIVE_PERMISSIONS,
            get(handlers::admin::effective_permissions),
        )
        .route(
            adm_paths::PATH_SEND_AS,
            get(handlers::admin::get_send_as).put(handlers::admin::set_send_as),
        )
        .route(
            adm_paths::PATH_GET_ACCOUNT_HASH,
            get(handlers::admin::get_account_with_hash),
        )
        .route(
            adm_paths::PATH_LIST_ACCOUNTS,
            get(handlers::admin::list_accounts).post(handlers::admin::add_account),
        )
        .route(
            adm_paths::PATH_UPDATE_ACCOUNT,
            put(handlers::admin::update_account).delete(handlers::admin::remove_account),
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
        .with_state(state.clone());

    // ── drafts ──────────────────────────────────────────────────────
    let drafts = Router::new()
        .route(
            adm_paths::PATH_LIST_DRAFTS,
            get(mailrs_core_sidestate::families::prefs::list_drafts::<CoreRpcState>)
                .post(mailrs_core_sidestate::families::prefs::save_draft::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_DELETE_DRAFT,
            delete(mailrs_core_sidestate::families::prefs::delete_draft::<CoreRpcState>),
        )
        .with_state(state.clone());

    // ── signatures ──────────────────────────────────────────────────
    let signatures = Router::new()
        .route(
            adm_paths::PATH_LIST_SIGNATURES,
            post(mailrs_core_sidestate::families::prefs::save_signature::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_DELETE_SIGNATURE,
            delete(mailrs_core_sidestate::families::prefs::delete_signature::<CoreRpcState>),
        )
        .with_state(state.clone());

    // ── webhooks ─────────────────────────────────────────────────────
    let webhooks = Router::new()
        .route(
            adm_paths::PATH_CREATE_WEBHOOK,
            post(mailrs_core_sidestate::families::admin_state::create_webhook::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_LIST_WEBHOOKS,
            get(mailrs_core_sidestate::families::admin_state::list_webhooks::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_DELETE_WEBHOOK,
            delete(mailrs_core_sidestate::families::admin_state::delete_webhook::<CoreRpcState>),
        )
        .with_state(state.clone());

    // ── templates ────────────────────────────────────────────────────
    let templates = Router::new()
        .route(
            adm_paths::PATH_LIST_TEMPLATES,
            get(mailrs_core_sidestate::families::prefs::list_templates::<CoreRpcState>)
                .post(mailrs_core_sidestate::families::prefs::save_template::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_DELETE_TEMPLATE,
            delete(mailrs_core_sidestate::families::prefs::delete_template::<CoreRpcState>),
        )
        .with_state(state.clone());

    // Authenticated subtree = everything except /v1/healthz + /v1/readyz.
    let authenticated = convo
        .merge(mb)
        .merge(th)
        .merge(super::router_views::view_routes(state.clone()))
        .merge(msg)
        .merge(adm)
        .merge(drafts)
        .merge(signatures)
        .merge(templates)
        .merge(webhooks);
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
