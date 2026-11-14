// CARVE-OUT: pure route registration; the builder chain is the table
//! The route table.
//!
//! Split out of `lib.rs` on 2026-08-02 — 479 lines of `.route(...)` in the middle of the
//! file that also held the handlers, the sweep and the ingest loop.
//!
//! Its own test is the point: a route string that fails to parse panics at
//! construction, which is process start, in production. 4,483 workspace
//! tests were green while not one of them built this table, and 2.19.0
//! went out with the REST API in a restart loop.

use mailrs_core_api::method::label as lb;

use mailrs_core_api::method::rethread;
use mailrs_core_routes::thread as shared;
use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::*;
use mailrs_core_api::method::import as import_paths;
use mailrs_core_api::method::message_index as mix;
use mailrs_core_api::method::report as rp;
use mailrs_core_api::method::senders as senders_paths;
use mailrs_core_api::method::sent;
use mailrs_core_api::method::shared as shared_paths;

mod directory_routes;
mod maintenance_routes;

/// The route table, with the business routes behind the bearer secret
/// in `MAILRS_CORE_API_SECRET` when one is set.
pub fn build_router(state: Arc<FastcoreState>) -> Router {
    let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV).unwrap_or_default();
    build_router_with_secret(state, secret)
}

/// Everything but `/v1/healthz` and `/v1/readyz` asks for
/// `Authorization: Bearer <secret>`, as the SQL core's table does. The
/// port is reachable from every container on the host network, and on
/// 2026-10-24 a web user reached it through an image proxy in webapi.
/// An empty secret leaves the routes open, for tests and local runs.
pub fn build_router_with_secret(state: Arc<FastcoreState>, secret: String) -> Router {
    let base = base_router(state.clone());
    // One Router for all business routes so matchit's trie sees the
    // full set at once. Earlier split into convo + thread Routers
    // hit a route-resolution bug where only the first-registered
    // route under /v1/users/{user}/conversations matched at runtime —
    // probable matchit collision between `conversations:list` (literal
    // ":list") and `conversations/categories` (path-separator). A
    // single Router with all routes registered side-by-side resolves it.
    let business = Router::new()
        .route(conv::PATH_LIST_CONVERSATIONS, post(list_conversations))
        .route(conv::PATH_SEARCH_CONVERSATIONS, post(search_conversations))
        .route(
            conv::PATH_CONVERSATIONS_BY_THREAD_IDS,
            post(conversations_by_thread_ids),
        )
        .route(conv::PATH_CONVERSATION_CATEGORIES, get(get_categories))
        .route(conv::PATH_UNSEEN_COUNT, get(get_unseen_count))
        .route(th::PATH_LIST_THREAD_MESSAGES, get(thread_messages))
        .route(th::PATH_LIST_SENT_MESSAGES, get(list_sent_messages))
        .route(rethread::PATH_SPLIT_MESSAGE, post(split_message_route))
        .route(th::PATH_DELIVER_MESSAGE, post(deliver_message))
        .route(
            senders_paths::PATH_RECLASSIFY_SENDER,
            post(crate::senders::reclassify),
        )
        .route(
            senders_paths::PATH_RESTORE_SENDER,
            post(crate::senders::restore),
        )
        .route(
            senders_paths::PATH_RETAIN_SENDER,
            post(crate::senders::retain),
        )
        .route(
            senders_paths::PATH_ACCEPT_SENDERS,
            post(crate::sections::accept_senders),
        )
        .route(shared_paths::PATH_ASSIGN, post(crate::shared::assign_route))
        .route(shared_paths::PATH_STATE, post(crate::shared::state_route))
        .route(
            shared_paths::PATH_CLAIM_REPLY,
            post(crate::shared::claim_route),
        )
        .route(
            shared_paths::PATH_RELEASE_REPLY,
            post(crate::shared::release_route),
        )
        .route(
            import_paths::PATH_IMPORT_MESSAGE,
            post(crate::import::import_message).layer(axum::extract::DefaultBodyLimit::max(
                import_paths::MAX_REQUEST_BYTES,
            )),
        )
        .route(
            sent::PATH_FILE_SENT,
            post(crate::sent::file_sent).layer(axum::extract::DefaultBodyLimit::max(
                sent::MAX_REQUEST_BYTES,
            )),
        )
        .route(th::PATH_MARK_READ, post(shared::mark_read::<FastcoreState>))
        .route(th::PATH_MARK_ALL_READ, post(mark_all_read_route))
        .route(th::PATH_MARK_LIST_READ, post(mark_list_read_route))
        .route(
            th::PATH_MARK_UNREAD,
            post(shared::mark_unread::<FastcoreState>),
        )
        .route(th::PATH_SNOOZE, put(shared::snooze::<FastcoreState>))
        .route(th::PATH_UNSNOOZE, delete(shared::unsnooze::<FastcoreState>))
        .route(th::PATH_PIN, post(shared::pin::<FastcoreState>))
        .route(th::PATH_UNPIN, post(shared::unpin::<FastcoreState>))
        .route(th::PATH_STAR, post(shared::star::<FastcoreState>))
        .route(th::PATH_UNSTAR, post(shared::unstar::<FastcoreState>))
        .route(th::PATH_QUARANTINE_LIST, get(list_quarantined))
        .route(th::PATH_QUARANTINE_RELEASE, post(release_quarantined))
        .route(th::PATH_THREAD_RELEASES, get(thread_releases))
        .route(th::PATH_FRAUD_VERDICT, get(get_fraud_verdict))
        .route(rp::PATH_REPORT_MESSAGE, post(crate::report::report))
        .route(rp::PATH_REPORT_OUTCOME, get(crate::report::outcome))
        .route(rp::PATH_PENDING_REPORTS, get(crate::report::pending))
        .route(rp::PATH_CONFIRM_REPORT, post(crate::report::confirm))
        .route(rp::PATH_DISMISS_REPORT, post(crate::report::dismiss))
        .route(th::PATH_ARCHIVE, post(shared::archive::<FastcoreState>))
        .route(th::PATH_UNARCHIVE, post(shared::unarchive::<FastcoreState>))
        .route(
            lb::PATH_LABELS,
            get(crate::routes::labels::list_labels).post(crate::routes::labels::create_label),
        )
        .route(
            lb::PATH_LABEL,
            post(crate::routes::labels::update_label).delete(crate::routes::labels::delete_label),
        )
        .route(
            lb::PATH_THREAD_LABEL,
            post(crate::routes::labels::label_thread).delete(crate::routes::labels::unlabel_thread),
        )
        .route(
            th::PATH_PREFETCH_IMAGES,
            post(crate::routes::images::prefetch_images),
        )
        .route(th::PATH_TRASH, post(crate::routes::trash::trash_thread))
        .route(th::PATH_UNTRASH, post(crate::routes::trash::untrash_thread))
        .route(
            th::PATH_EMPTY_TRASH,
            post(crate::routes::trash::empty_trash),
        )
        .route(th::PATH_MARK_JUNK, post(mark_junk))
        .route(th::PATH_MARK_NOT_JUNK, post(mark_not_junk))
        .route(th::PATH_MARK_NOTIFICATION, post(mark_notification))
        .route(th::PATH_MARK_PROMOTION, post(mark_promotion))
        .route(th::PATH_MOVE_TO_INBOX, post(move_to_inbox))
        .route(th::PATH_DELETE_THREAD, delete(delete_thread))
        .route(adm::PATH_GET_ACCOUNT_HASH, get(get_account_with_hash))
        .route(adm::PATH_EFFECTIVE_PERMISSIONS, get(effective_permissions))
        .route(adm::PATH_SEND_AS, get(get_send_as).put(set_send_as))
        .route(
            adm::PATH_LIST_ACCOUNTS,
            get(list_accounts).post(add_account_route),
        )
        .route(
            adm::PATH_UPDATE_ACCOUNT,
            put(update_account_route).delete(remove_account_route),
        )
        .route(adm::PATH_RESTORE_ACCOUNT, post(restore_account_route))
        .route(adm::PATH_SET_QUOTA, post(set_quota_route))
        .route(
            adm::PATH_UPDATE_RECOVERY_EMAIL,
            post(set_recovery_email_route),
        )
        .route(adm::PATH_SET_ACCOUNT_PASSWORD, post(set_password_route))
        .route(adm::PATH_CLEAR_ACCOUNT_PASSWORD, post(clear_password_route))
        // the operator's clearing, on the host only: see password_clear
        .route(
            adm::PATH_OPERATOR_PASSWORD_CLEAR,
            post(mailrs_core_sidestate::families::password_clear::operator_clear::<FastcoreState>),
        )
        .route(
            adm::PATH_OPERATOR_PASSWORD_CLEAR_PREVIEW,
            post(
                mailrs_core_sidestate::families::password_clear::operator_preview::<FastcoreState>,
            ),
        )
        .route(
            adm::PATH_OWNERLESS_THREADS,
            get(crate::routes::ownerless::list),
        )
        .route(
            adm::PATH_DELETE_OWNERLESS_THREAD,
            post(crate::routes::ownerless::delete),
        )
        .route(
            adm::PATH_REJUDGE_BATCHES,
            get(crate::rejudge::routes::batches),
        )
        .route(
            adm::PATH_RELEASE_REJUDGE_BATCH,
            post(crate::rejudge::routes::release),
        )
        .route(
            adm::PATH_CONFIRM_REJUDGE_BATCH,
            post(crate::rejudge::routes::confirm),
        )
        .route(
            adm::PATH_DISCARD_REJUDGE_BATCH,
            post(crate::rejudge::routes::discard),
        )
        .route(adm::PATH_SET_MESSAGE_FLAGS, post(set_message_flags_route))
        // Ops endpoint — reset every user's ingest cursor to 0 so the
        // next sync tick re-processes historic threads and (via the
        // Group F diff path) backfills messages fastcore missed under
        // the older "skip-existing" ingest behaviour.
        .route(
            "/v1/admin/sync/reset-cursors",
            post(reset_sync_cursors_route),
        )
        .route(mb::PATH_LIST_MAILBOXES, get(list_mailboxes))
        .route(
            msg::PATH_GET_MESSAGE_BY_UID_USER,
            get(get_message_by_uid_for_user),
        )
        .route(msg::PATH_GET_INVITE, get(get_invite))
        .route(mix::PATH_MESSAGE_INDEX, get(message_index))
        // ── shared side-state (network kevy): drafts / signatures /
        // templates — same keys webapi + pg-core read (v2 point 3) ──
        .route(
            adm::PATH_LIST_DRAFTS,
            get(mailrs_core_sidestate::families::prefs::list_drafts::<FastcoreState>)
                .post(mailrs_core_sidestate::families::prefs::save_draft::<FastcoreState>),
        )
        .route(
            adm::PATH_DELETE_DRAFT,
            delete(mailrs_core_sidestate::families::prefs::delete_draft::<FastcoreState>),
        )
        .route(
            adm::PATH_LIST_SIGNATURES,
            post(mailrs_core_sidestate::families::prefs::save_signature::<FastcoreState>),
        )
        .route(
            adm::PATH_DELETE_SIGNATURE,
            delete(mailrs_core_sidestate::families::prefs::delete_signature::<FastcoreState>),
        )
        .route(
            adm::PATH_LIST_TEMPLATES,
            get(mailrs_core_sidestate::families::prefs::list_templates::<FastcoreState>)
                .post(mailrs_core_sidestate::families::prefs::save_template::<FastcoreState>),
        )
        .route(
            adm::PATH_DELETE_TEMPLATE,
            delete(mailrs_core_sidestate::families::prefs::delete_template::<FastcoreState>),
        )
        .route(
            adm::PATH_CREATE_WEBHOOK,
            post(mailrs_core_sidestate::families::admin_state::create_webhook::<FastcoreState>),
        )
        .route(
            adm::PATH_LIST_WEBHOOKS,
            get(mailrs_core_sidestate::families::admin_state::list_webhooks::<FastcoreState>),
        )
        .route(
            adm::PATH_DELETE_WEBHOOK,
            delete(mailrs_core_sidestate::families::admin_state::delete_webhook::<FastcoreState>),
        )
        .route(
            adm::PATH_LIST_DOMAINS,
            get(routes::mail_admin::list_domains).post(routes::mail_admin::add_domain),
        )
        .route(
            adm::PATH_REMOVE_DOMAIN,
            delete(routes::mail_admin::remove_domain),
        )
        .route(
            msg::PATH_FIND_BY_MESSAGE_ID,
            get(routes::message::find_by_message_id),
        );
    // Before `with_state`: the maintenance routes take the same state,
    // and applying it first would leave them nothing to attach to.
    let business = maintenance_routes::maintenance_routes(business);
    let sink = crate::thread_events::sink(state.clone());
    let business = directory_routes::directory_routes(business)
        .with_state(state)
        .route_layer(axum::middleware::from_fn_with_state(
            sink,
            mailrs_core_api::server::thread_events,
        ));

    if secret.is_empty() {
        tracing::warn!(
            event = "core_rpc_no_auth",
            "MAILRS_CORE_API_SECRET unset — core RPC will accept unauthenticated requests"
        );
        return base.merge(business);
    }
    base.merge(business.layer(axum::middleware::from_fn_with_state(
        Arc::new(secret),
        mailrs_core_api::server::auth_middleware,
    )))
}
