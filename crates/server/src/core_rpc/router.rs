//! The core-RPC route table.
//!
//! Split out of `core_rpc/mod.rs` on 2026-08-02. `route_parity_lock` in
//! `tests/parity.rs` is what holds this table to the fastcore lane's —
//! the two cores serve one client, so a path here and not there is a
//! feature that works or 404s depending on which is deployed.

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
            get(mailrs_core_sidestate::families::groups_admin::get_api_key_by_prefix::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_TOUCH_API_KEY,
            post(mailrs_core_sidestate::families::groups_admin::touch_api_key::<CoreRpcState>),
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
            get(mailrs_core_sidestate::families::groups_admin::get_sieve::<CoreRpcState>)
                .post(handlers::admin::set_sieve)
                .delete(handlers::admin::delete_sieve),
        )
        // audit log
        .route(
            adm_paths::PATH_LIST_AUDIT_LOG,
            get(mailrs_core_sidestate::families::admin_state::list_audit_log::<CoreRpcState>).post(mailrs_core_sidestate::families::admin_state::log_audit::<CoreRpcState>),
        )
        // groups + permissions
        .route(
            adm_paths::PATH_LIST_GROUPS,
            get(mailrs_core_sidestate::families::groups_admin::list_groups::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_GET_GROUP_PERMISSIONS,
            get(mailrs_core_sidestate::families::groups_admin::get_group_permissions::<CoreRpcState>).put(mailrs_core_sidestate::families::groups_admin::set_group_permissions::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_LIST_GROUP_MEMBERS,
            get(mailrs_core_sidestate::families::groups_admin::list_group_members::<CoreRpcState>).post(mailrs_core_sidestate::families::groups_admin::add_account_to_group::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_REMOVE_ACCOUNT_FROM_GROUP,
            delete(mailrs_core_sidestate::families::groups_admin::remove_account_from_group::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_GET_ACCOUNT_GROUPS,
            get(mailrs_core_sidestate::families::groups_admin::get_account_groups::<CoreRpcState>),
        )
        .with_state(state.clone());

    // ── analysis ─────────────────────────────────────────────────────
    let anal = Router::new()
        .route(
            analysis_paths::PATH_GET_ANALYSIS,
            get(mailrs_core_sidestate::families::analysis::get_analysis::<CoreRpcState>),
        )
        .route(
            analysis_paths::PATH_COUNT_UNANALYZED,
            get(mailrs_core_sidestate::families::analysis::count_unanalyzed::<CoreRpcState>),
        )
        .route(
            analysis_paths::PATH_BOOST_IMPORTANCE,
            post(mailrs_core_sidestate::families::analysis::boost_importance::<CoreRpcState>),
        )
        .route(
            analysis_paths::PATH_ATTACHMENT_TEXTS,
            get(mailrs_core_sidestate::families::analysis::attachment_texts::<CoreRpcState>),
        )
        .route(
            analysis_paths::PATH_SEMANTIC_SEARCH,
            post(mailrs_core_sidestate::families::analysis::semantic_search),
        )
        .with_state(state.clone());

    // ── contacts ─────────────────────────────────────────────────────
    let ct = Router::new()
        .route(
            contact_paths::PATH_SEARCH_CONTACTS,
            get(mailrs_core_sidestate::families::contacts::search_contacts::<CoreRpcState>),
        )
        .route(
            contact_paths::PATH_UPSERT_INBOUND,
            post(mailrs_core_sidestate::families::contacts::upsert_inbound::<CoreRpcState>),
        )
        .route(
            contact_paths::PATH_CONTACT_SCORING,
            get(mailrs_core_sidestate::families::contacts::contact_scoring::<CoreRpcState>),
        )
        .route(
            contact_paths::PATH_HAS_SENT_TO,
            get(mailrs_core_sidestate::families::contacts::has_sent_to::<CoreRpcState>),
        )
        .route(
            contact_paths::PATH_SENDER_FEEDBACK,
            post(mailrs_core_sidestate::families::contacts::sender_feedback::<CoreRpcState>),
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
            get(mailrs_core_sidestate::families::prefs::list_signatures::<CoreRpcState>)
                .post(mailrs_core_sidestate::families::prefs::save_signature::<CoreRpcState>),
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

    // ── reactions ────────────────────────────────────────────────────
    let rx = Router::new()
        .route(
            adm_paths::PATH_GET_THREAD_REACTIONS,
            get(mailrs_core_sidestate::families::admin_state::get_thread_reactions::<CoreRpcState>),
        )
        .route(
            adm_paths::PATH_TOGGLE_REACTION,
            put(mailrs_core_sidestate::families::admin_state::toggle_reaction::<CoreRpcState>),
        )
        .with_state(state.clone());

    // ── outbound (sender ↔ core) ─────────────────────────────────────
    let ob = Router::new()
        .route(
            ob_paths::PATH_ENQUEUE,
            post(mailrs_core_sidestate::families::outbound::enqueue::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_CLAIM,
            post(mailrs_core_sidestate::families::outbound::claim::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_STATS,
            get(mailrs_core_sidestate::families::outbound::stats::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_RECOVER_STALE,
            post(mailrs_core_sidestate::families::outbound::recover_stale::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_MARK_DELIVERED,
            post(mailrs_core_sidestate::families::outbound::mark_delivered::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_MARK_FAILED,
            post(mailrs_core_sidestate::families::outbound::mark_failed::<CoreRpcState>),
        )
        .route(
            ob_paths::PATH_MARK_BOUNCED,
            post(mailrs_core_sidestate::families::outbound::mark_bounced::<CoreRpcState>),
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
