//! mailrs-webapi — REST + MCP + JMAP + CalDAV/CardDAV frontend.
//!
//! Phase 3 of the 4-process split (checklist
//! `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §3).
//!
//! This crate is currently a scaffold — no routes mounted yet. Subsequent
//! loops fill in the REST and MCP handlers by copying the existing
//! `crates/server/src/web/` + `crates/server/src/mcp/` trees and replacing
//! `state.mailbox.X()` / `state.domain.X()` direct calls with
//! `state.core.X()` RPC client calls.
//!
//! Boot order:
//! 1. tracing init
//! 2. config from env (MAILRS_CORE_RPC_BASE / MAILRS_CORE_API_SECRET /
//!    MAILRS_KEVY_URL / MAILRS_WEB_BIND etc.)
//! 3. mailrs-core-api client
//! 4. kevy_net client (for session store + cache bust)
//! 5. meili client (for search)
//! 6. axum router + listen
//! 7. signal handler

#![allow(missing_docs)]

pub mod handlers;
pub mod session;

use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Shared state injected into every web handler.
///
/// Distinct from the old `crate::server::web::WebState` — fewer fields
/// because PG/mailbox/domain backings now sit behind `core_client`.
pub struct WebState {
    /// The ONE core-api RPC client. Points at whichever serving core is
    /// running (fastcore/kevy OR core/pg-spg) via `MAILRS_CORE_RPC_BASE`
    /// — webapi is 100% agnostic to which backend answers. The switch
    /// boundary is exactly this env var; there is no per-route client
    /// selection and no backend conditional anywhere above this field
    /// (v2 dual-mode: RFC lazy-wobbling-nebula).
    pub core: Arc<mailrs_core_api::client::Client>,
    /// Process bind address for the public REST/MCP listener.
    pub bind_addr: String,
    /// Shared WS broadcast bus, initialized lazily on the first
    /// `/api/events` upgrade. Held here so all WS clients share
    /// a single kevy subscribe loop.
    pub event_bus: std::sync::OnceLock<handlers::events::EventBus>,
    /// Wall-clock start of this webapi process, used to compute the
    /// `uptime_secs` field surfaced by `/api/health` + `/api/status`.
    /// UI status bars and SMTP-monitor cards read this to render a
    /// live uptime badge; before it existed both endpoints returned
    /// no uptime field and the frontend rendered `NaN`.
    pub started_at: std::time::Instant,
}

impl WebState {
    /// Build state from env. Panics if `MAILRS_CORE_API_SECRET` is missing.
    pub fn from_env() -> Self {
        let base = std::env::var("MAILRS_CORE_RPC_BASE")
            .unwrap_or_else(|_| "http://localhost:3300".into());
        let secret = std::env::var(mailrs_core_api::AUTH_SECRET_ENV)
            .expect("MAILRS_CORE_API_SECRET required for webapi");
        let core = Arc::new(mailrs_core_api::client::Client::new(base, secret));
        let bind_addr = std::env::var("MAILRS_WEB_BIND").unwrap_or_else(|_| "0.0.0.0:3100".into());
        Self {
            core,
            bind_addr,
            event_bus: std::sync::OnceLock::new(),
            started_at: std::time::Instant::now(),
        }
    }
}

/// Build the axum router. Conversation routes wired (Phase 3.5);
/// auth + rest fill in next loops.
pub fn build_router(state: Arc<WebState>) -> axum::Router {
    use axum::routing::{get, post};
    use handlers::conversations as c;
    let _ = stub_auth_middleware; // kept for tests / dev mode reference

    use axum::routing::put;
    let convo = axum::Router::new()
        .route("/api/conversations", get(c::get_conversations))
        .route("/api/conversations/batch", post(c::batch_mutation))
        .route("/api/conversations/mark-all-read", post(c::mark_all_read))
        .route("/api/conversations/categories", get(c::get_categories))
        .route("/api/conversations/unseen-count", get(c::get_unseen_count))
        .route(
            "/api/conversations/{thread_id}/read",
            post(c::mark_thread_read),
        )
        .route(
            "/api/conversations/{thread_id}/unread",
            post(c::mark_thread_unread),
        )
        .route("/api/conversations/{thread_id}/star", post(c::star_thread))
        .route(
            "/api/conversations/{thread_id}/unstar",
            post(c::unstar_thread),
        )
        .route("/api/conversations/{thread_id}/pin", post(c::pin_thread))
        .route(
            "/api/conversations/{thread_id}/unpin",
            post(c::unpin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/archive",
            post(c::archive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unarchive",
            post(c::unarchive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/mark-junk",
            post(c::mark_junk),
        )
        .route(
            "/api/conversations/{thread_id}/mark-not-junk",
            post(c::mark_not_junk),
        )
        .route(
            "/api/conversations/{thread_id}/mark-notification",
            post(c::mark_notification),
        )
        .route(
            "/api/conversations/{thread_id}/mark-promotion",
            post(c::mark_promotion),
        )
        .route(
            "/api/conversations/{thread_id}/move-to-inbox",
            post(c::move_to_inbox),
        )
        .route(
            "/api/conversations/{thread_id}/snooze",
            put(c::snooze_thread).delete(c::unsnooze_thread),
        )
        .route("/api/mail/sent", get(c::list_sent_messages))
        .route(
            "/api/conversations/{thread_id}",
            get(c::get_thread_messages).delete(c::delete_thread),
        )
        .route(
            "/api/conversations/{thread_id}/reactions",
            get(handlers::mail::get_thread_reactions),
        )
        .route(
            "/api/conversations/{thread_id}/messages/{uid}/reactions",
            put(handlers::mail::toggle_reaction),
        );

    // Phase 12b — all /api/mail/* + BIMI + proxy routes are now
    // fastcore-native (kevy + maildir + external HTTP). Zero spg touch.
    use axum::routing::delete;
    let mail = axum::Router::new()
        .route("/api/mail/folders", get(handlers::mail::get_folders))
        .route(
            "/api/mail/messages/{uid}/raw",
            get(handlers::messages::get_message_raw),
        )
        .route(
            "/api/mail/messages/{uid}/attachments/{index}",
            get(handlers::messages::get_attachment),
        )
        .route(
            "/api/mail/messages/{uid}/attachments/{index}/content",
            get(handlers::messages::get_attachment_content),
        )
        .route(
            "/api/mail/messages/{uid}/flags",
            post(handlers::messages::update_flags),
        )
        .route(
            "/api/mail/inline-upload",
            post(handlers::inline::inline_upload),
        )
        .route("/api/mail/inline/{id}", get(handlers::inline::get_inline))
        .route(
            "/api/mail/keys",
            get(handlers::misc::get_keys).post(handlers::misc::save_key),
        )
        .route(
            "/api/mail/check-deliverability",
            get(handlers::misc::check_deliverability),
        )
        .route(
            "/api/mail/spam-feedback",
            post(handlers::misc::spam_feedback),
        )
        .route("/api/mail/export", get(handlers::misc::export_mbox))
        .route(
            "/api/conversations/search",
            get(handlers::misc::search_conversations),
        )
        .route("/api/mail/send", post(handlers::prefs::send_message))
        .route(
            "/api/mail/send-multipart",
            post(handlers::prefs::send_message_multipart),
        )
        .route("/api/queue", get(handlers::prefs::get_queue_stats))
        .route("/api/contacts", get(handlers::prefs::get_contacts))
        .route("/api/mail/feedback", post(handlers::prefs::submit_feedback))
        // v2.4.1 Phase 3 (RFC-B §3.5) — per-user sender allow/block
        .route(
            "/api/spam/whitelist",
            get(handlers::spam_lists::list_whitelist).post(handlers::spam_lists::add_whitelist),
        )
        .route(
            "/api/spam/whitelist/{address}",
            delete(handlers::spam_lists::remove_whitelist),
        )
        .route(
            "/api/spam/blacklist",
            get(handlers::spam_lists::list_blacklist).post(handlers::spam_lists::add_blacklist),
        )
        .route(
            "/api/spam/blacklist/{address}",
            delete(handlers::spam_lists::remove_blacklist),
        )
        .route(
            "/api/mail/drafts",
            get(handlers::prefs::list_drafts).post(handlers::prefs::save_draft),
        )
        .route(
            "/api/mail/drafts/{id}",
            delete(handlers::prefs::delete_draft),
        )
        .route(
            "/api/mail/signatures",
            get(handlers::prefs::list_signatures).post(handlers::prefs::save_signature),
        )
        .route(
            "/api/mail/signatures/{id}",
            delete(handlers::prefs::delete_signature),
        )
        .route(
            "/api/mail/templates",
            get(handlers::prefs::list_templates).post(handlers::prefs::save_template),
        )
        .route(
            "/api/mail/templates/{id}",
            delete(handlers::prefs::delete_template),
        )
        .route("/api/bimi/{domain}", get(handlers::prefs::get_bimi))
        .route("/api/icon/{domain}", get(handlers::icon::get_icon))
        .route("/api/proxy/image", get(handlers::prefs::proxy_image))
        .route("/api/proxy/link", get(handlers::prefs::proxy_link))
        // Phase 13 — remaining route coverage.
        .route("/api/mail/stats", get(handlers::complete::get_mail_stats))
        .route(
            "/api/mail/messages/{uid}",
            get(handlers::complete::get_message_single),
        )
        .route(
            "/api/mail/keys/status",
            get(handlers::complete::keys_status),
        )
        .route(
            "/api/auth/recovery-email",
            get(handlers::complete::get_recovery_email)
                .post(handlers::complete::set_recovery_email),
        )
        .route(
            "/api/auth/totp/status",
            get(handlers::complete::totp_status),
        )
        .route("/api/auth/totp/setup", post(handlers::complete::totp_setup))
        .route(
            "/api/auth/totp/enable",
            post(handlers::complete::totp_enable),
        )
        .route(
            "/api/auth/totp/disable",
            post(handlers::complete::totp_disable),
        )
        .route(
            "/api/queue/{id}/retry",
            post(handlers::complete::queue_retry),
        )
        .route(
            "/api/calendar/feeds",
            get(handlers::calendar::list_feeds).post(handlers::calendar::create_feed),
        )
        .route(
            "/api/calendar/feeds/{feed_id}",
            delete(handlers::calendar::delete_feed),
        )
        .route(
            "/api/calendar/conflicts",
            get(handlers::calendar::get_conflicts),
        )
        .route(
            "/api/invites/{message_id}/rsvp",
            post(handlers::invites::submit_rsvp),
        )
        .route(
            "/api/invites/{message_id}/counter",
            post(handlers::invites::submit_counter),
        )
        .route(
            "/api/conversations/semantic-search",
            get(handlers::search::semantic_search),
        )
        .route(
            "/api/mail/pending/{message_id}",
            delete(handlers::messages::cancel_pending_send),
        )
        // G13.3 scheduled outbound queue control
        .route(
            "/api/scheduled/{id}/cancel",
            post(handlers::messages::cancel_scheduled),
        )
        .route(
            "/api/scheduled/{id}/reschedule",
            post(handlers::messages::reschedule_scheduled),
        )
        .route(
            "/api/mail/messages/{uid}",
            delete(handlers::messages::delete_message),
        )
        .route(
            "/api/mail/folders/{name}/messages",
            get(handlers::mail::list_folder_messages),
        )
        .route(
            "/api/mail/keys/{key_type}",
            get(handlers::keys::get_key)
                .put(handlers::keys::set_key)
                .delete(handlers::keys::delete_key),
        )
        .route(
            "/api/agent/keys",
            get(handlers::complete::list_agent_keys).post(handlers::complete::create_agent_key),
        )
        .route(
            "/api/agent/keys/{id}",
            delete(handlers::complete::delete_agent_key),
        )
        .route(
            "/api/agent/webhooks",
            get(handlers::complete::list_agent_webhooks)
                .post(handlers::complete::create_agent_webhook),
        )
        .route(
            "/api/agent/webhooks/{id}",
            delete(handlers::complete::delete_agent_webhook),
        )
        .route(
            "/api/admin/apps",
            get(handlers::complete::list_apps).post(handlers::complete::create_app),
        )
        .route(
            "/api/admin/apps/{app_id}",
            get(handlers::complete::get_app).delete(handlers::complete::delete_app),
        )
        .route(
            "/api/admin/audit/accounts",
            get(handlers::complete::audit_accounts),
        )
        .route(
            "/api/admin/audit/conversations",
            get(handlers::complete::audit_conversations),
        )
        .route(
            "/api/admin/audit/conversations/{thread_id}",
            get(handlers::complete::audit_conversation_detail),
        )
        .route(
            "/api/admin/audit/conversations/{thread_id}/messages",
            get(handlers::complete::audit_conversation_messages),
        )
        .route(
            "/api/admin/audit/messages/{uid}/raw",
            get(handlers::complete::audit_message_raw),
        )
        .route(
            "/api/admin/config/smtp",
            get(handlers::complete::get_smtp_config).post(handlers::complete::set_smtp_config),
        )
        .route(
            "/api/admin/system-config",
            get(handlers::complete::get_system_config),
        )
        .route(
            "/api/admin/system-config/{key}",
            post(handlers::complete::set_system_config_key),
        )
        .route(
            "/api/admin/groups",
            get(handlers::complete::list_groups).post(handlers::complete::create_group),
        )
        .route(
            "/api/admin/groups/{id}",
            delete(handlers::complete::delete_group),
        )
        .route(
            "/api/admin/groups/{id}/permissions",
            get(handlers::complete::get_group_permissions)
                .post(handlers::complete::set_group_permissions),
        )
        .route(
            "/api/admin/groups/{id}/members",
            get(handlers::complete::list_group_members).post(handlers::complete::add_group_member),
        )
        .route(
            "/api/admin/groups/{id}/members/{address}",
            delete(handlers::complete::remove_group_member),
        )
        .route(
            "/api/admin/permissions",
            get(handlers::complete::list_permissions),
        )
        .route(
            "/api/admin/email-groups",
            get(handlers::complete::list_email_groups).post(handlers::complete::create_email_group),
        )
        .route(
            "/api/admin/email-groups/{id}",
            delete(handlers::complete::delete_email_group),
        )
        .route(
            "/api/admin/greylist/local-lists",
            get(handlers::complete::list_greylist_local)
                .post(handlers::complete::create_greylist_entry),
        )
        .route(
            "/api/admin/greylist/local-lists/{id}",
            delete(handlers::complete::delete_greylist_entry),
        )
        .route(
            "/api/admin/queues",
            get(handlers::complete::list_admin_queue),
        );

    let auth_routes = axum::Router::new()
        .route("/api/auth/me", get(handlers::auth::auth_me))
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route(
            "/api/auth/change-password",
            post(handlers::auth::change_password),
        )
        .route("/api/auth/verify", post(handlers::auth::verify_credentials))
        .route("/api/auth/verify-totp", post(handlers::auth::verify_totp))
        // OIDC provider auth-required endpoints.
        .route("/oauth/authorize", get(handlers::oidc::authorize))
        .route("/api/auth/oidc/login", get(handlers::oidc::oidc_login));

    // JMAP endpoints (authenticated).
    let jmap_routes = axum::Router::new()
        .route("/.well-known/jmap", get(handlers::jmap::jmap_session))
        .route("/jmap", post(handlers::jmap::jmap_api))
        .route("/jmap/eventsource/", get(handlers::jmap::jmap_eventsource));

    // DAV endpoints (authenticated). CalDAV / CardDAV clients drive
    // discovery with OPTIONS / PROPFIND / REPORT — axum's
    // MethodRouter only routes standard verbs, so collection routes
    // use `any(...)` so every method (including PROPFIND / REPORT /
    // MKCALENDAR) lands on the same handler which inspects the
    // Method header. Leaf item routes stick to PUT/GET/DELETE.
    use axum::routing::any;
    let dav_routes = axum::Router::new()
        .route("/dav/", any(handlers::dav::dav_root))
        .route("/dav/principals/{user}/", any(handlers::dav::dav_principal))
        .route(
            "/dav/calendars/{user}/",
            any(handlers::dav::calendars_collection),
        )
        .route(
            "/dav/addressbooks/{user}/",
            any(handlers::dav::addressbooks_collection),
        )
        .route(
            "/dav/calendars/{user}/{cal}/{uid}",
            put(handlers::dav::put_calendar_event)
                .get(handlers::dav::get_calendar_event)
                .delete(handlers::dav::delete_calendar_event),
        )
        .route(
            "/dav/addressbooks/{user}/{book}/{uid}",
            put(handlers::dav::put_contact)
                .get(handlers::dav::get_contact)
                .delete(handlers::dav::delete_contact),
        );

    let admin_routes = axum::Router::new()
        .route(
            "/api/admin/accounts",
            get(handlers::admin::list_accounts).post(handlers::admin::add_account),
        )
        .route(
            "/api/admin/accounts/{address}",
            delete(handlers::admin::remove_account).put(handlers::admin::update_account),
        )
        .route(
            "/api/admin/accounts/{address}/quota",
            get(handlers::admin::get_account_quota).post(handlers::admin::set_account_quota),
        )
        .route(
            "/api/admin/accounts/{address}/sieve",
            get(handlers::admin::get_account_sieve)
                .post(handlers::admin::set_account_sieve)
                .delete(handlers::admin::delete_account_sieve),
        )
        .route(
            "/api/admin/accounts/{address}/groups",
            get(handlers::admin::list_account_groups),
        )
        .route(
            "/api/admin/accounts/{address}/overrides",
            get(handlers::admin::get_account_overrides).put(handlers::admin::set_account_overrides),
        )
        .route(
            "/api/admin/domains/{name}/check",
            post(handlers::admin::check_domain_dns),
        )
        .route(
            "/api/admin/reconcile-maildir",
            post(handlers::admin::reconcile_maildir),
        )
        .route(
            "/api/admin/suppressions",
            get(handlers::admin::list_suppressions).delete(handlers::admin::clear_suppressions),
        )
        .route(
            "/api/admin/email-groups/{id}/members",
            get(handlers::admin::list_email_group_members)
                .post(handlers::admin::add_email_group_member),
        )
        .route(
            "/api/admin/email-groups/{id}/members/{address}",
            delete(handlers::admin::remove_email_group_member),
        )
        .route(
            "/api/admin/apps/{app_id}/scopes",
            put(handlers::admin::set_app_scopes),
        )
        .route(
            "/api/admin/cache/flush-conversations",
            post(handlers::admin::flush_conversations_cache),
        )
        .route(
            "/api/admin/rbl-status",
            get(handlers::admin::get_rbl_status),
        )
        .route(
            "/api/admin/reputation",
            get(handlers::admin::get_reputation),
        )
        .route(
            "/api/admin/spam-feedback-stats",
            get(handlers::admin::get_spam_feedback_stats),
        )
        .route(
            "/api/admin/aliases",
            get(handlers::admin::list_aliases).post(handlers::admin::add_alias),
        )
        .route(
            "/api/admin/aliases/{id}",
            delete(handlers::admin::remove_alias),
        )
        .route(
            "/api/admin/domains",
            get(handlers::admin::list_domains).post(handlers::admin::add_domain),
        )
        .route(
            "/api/admin/domains/{name}",
            delete(handlers::admin::remove_domain),
        )
        .route(
            "/api/admin/webhook-subscriptions",
            post(handlers::admin::create_webhook),
        )
        .route(
            "/api/admin/webhook-subscriptions/{id}",
            delete(handlers::admin::delete_webhook),
        )
        .route(
            "/api/admin/accounts/{address}/webhook-subscriptions",
            get(handlers::admin::list_webhooks),
        )
        .route("/api/admin/audit-log", get(handlers::admin::list_audit_log))
        .route(
            "/api/admin/audit-log/export",
            get(handlers::admin::export_audit_log),
        )
        .route("/api/admin/export", get(handlers::admin::admin_export))
        .route(
            "/api/admin/oauth-clients",
            get(handlers::oidc::list_oauth_clients).post(handlers::oidc::create_oauth_client),
        )
        .route(
            "/api/admin/oauth-clients/{client_id}",
            delete(handlers::oidc::delete_oauth_client),
        );

    // Phase 3.9 — real session auth via kevy when MAILRS_KEVY_URL is set;
    // falls back to the X-Mailrs-User header in dev (no kevy) mode.
    let authenticated = convo
        .merge(mail)
        .merge(auth_routes)
        .merge(admin_routes)
        .merge(jmap_routes)
        .merge(dav_routes)
        // admin gate — 403 unless caller has admin.* permission or is_super.
        // Runs after session_auth (below) so authed user is in Extensions.
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            handlers::kevy_util::admin_middleware,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session::session_auth_middleware,
        ));

    // Unauthenticated routes — login + health. login intentionally
    // sits outside session_auth_middleware so a freshly-arrived client
    // (no session yet) can establish one.
    let unauth = axum::Router::new()
        .route("/_health", get(health_handler))
        .route("/api/health", get(health_handler))
        .route("/api/readiness", get(readiness_handler))
        .route("/api/status", get(status_handler))
        .route("/api/auth/login", post(handlers::auth::login))
        .route(
            "/api/auth/oidc/config",
            get(handlers::complete::oidc_config),
        )
        .route(
            "/api/auth/forgot-password",
            post(handlers::complete::forgot_password),
        )
        .route(
            "/api/auth/reset-password",
            post(handlers::complete::reset_password),
        )
        // WS upgrade uses `?token=<hex>` from query — browsers can't
        // set custom headers on WebSocket. Auth is inside the handler
        // (checks kevy `session:<token>` directly).
        .route("/api/events", get(handlers::events::ws_events))
        // Prometheus, unauth on internal network.
        .route("/metrics", get(handlers::metrics::prometheus_metrics))
        // Public-key lookup by address — unauth (used by any correspondent).
        .route(
            "/api/keys/{address}/pgp",
            get(handlers::keys::get_public_pgp_key),
        )
        .route(
            "/api/keys/{address}/smime",
            get(handlers::keys::get_public_smime_key),
        )
        // Autodiscover / autoconfig / mta-sts (unauth).
        .route(
            "/autodiscover/autodiscover.xml",
            get(handlers::autodiscover::autodiscover_outlook)
                .post(handlers::autodiscover::autodiscover_outlook),
        )
        .route(
            "/Autodiscover/Autodiscover.xml",
            get(handlers::autodiscover::autodiscover_outlook)
                .post(handlers::autodiscover::autodiscover_outlook),
        )
        .route(
            "/.well-known/autoconfig/mail/config-v1.1.xml",
            get(handlers::autodiscover::autoconfig_mozilla),
        )
        .route(
            "/.well-known/apple-mobileconfig",
            get(handlers::autodiscover::apple_mobileconfig),
        )
        .route(
            "/.well-known/mta-sts.txt",
            get(handlers::autodiscover::mta_sts_policy),
        )
        // OIDC discovery + JWKS + provider endpoints (unauth).
        .route(
            "/.well-known/openid-configuration",
            get(handlers::oidc::openid_configuration),
        )
        .route("/.well-known/jwks.json", get(handlers::oidc::jwks))
        .route("/oauth/token", post(handlers::oidc::token))
        .route("/oauth/userinfo", get(handlers::oidc::userinfo))
        // External IdP callback (kicks off session via redirect).
        .route(
            "/api/auth/oidc/callback",
            get(handlers::oidc::oidc_callback),
        )
        // DAV well-known redirects (unauth — DAV spec allows anonymous discovery).
        .route("/.well-known/caldav", get(handlers::dav::well_known_caldav))
        .route(
            "/.well-known/carddav",
            get(handlers::dav::well_known_carddav),
        );

    // Match monolith's 25 MiB multipart cap. Axum's default is 2 MiB,
    // which trips /api/mail/send-multipart on any attached file bigger
    // than that — the UI just shows "Send failed" with no server-side
    // trace. See crates/server/src/web/mod.rs:MAX_MULTIPART_BODY.
    const MAX_MULTIPART_BODY: usize = 25 * 1024 * 1024;

    // MCP Streamable HTTP surface at /mcp. Runs its own auth
    // middleware (task-local user) so rmcp's session factory sees
    // the caller. Mounted OUTSIDE the REST auth stack because rmcp
    // manages its own Extension shape.
    let mcp = handlers::mcp::mcp_router(state.clone()).route_layer(
        axum::middleware::from_fn_with_state(state.clone(), handlers::mcp::mcp_auth_middleware),
    );

    let mut app = unauth
        .merge(authenticated)
        .merge(mcp)
        .layer(axum::extract::DefaultBodyLimit::max(MAX_MULTIPART_BODY))
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(
                    tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
                )
                .on_response(
                    tower_http::trace::DefaultOnResponse::new().level(tracing::Level::INFO),
                )
                .on_failure(tower_http::trace::DefaultOnFailure::new().level(tracing::Level::WARN)),
        )
        .with_state(state);

    // Serve the React UI from `MAILRS_WEB_STATIC_DIR` (defaults to
    // `/opt/mailrs/web` to match the monolith's bind-mount layout).
    // SPA fallback: any non-API path serves index.html so client-side
    // routing works.
    let static_dir =
        std::env::var("MAILRS_WEB_STATIC_DIR").unwrap_or_else(|_| "/opt/mailrs/web".to_string());
    if std::path::Path::new(&static_dir)
        .join("index.html")
        .exists()
    {
        use tower_http::services::{ServeDir, ServeFile};
        let index = format!("{static_dir}/index.html");
        app = app.fallback_service(ServeDir::new(&static_dir).fallback(ServeFile::new(index)));
        tracing::info!(dir = %static_dir, "webapi serving static UI");
    } else {
        tracing::info!(
            dir = %static_dir,
            "MAILRS_WEB_STATIC_DIR missing index.html — webapi will 404 non-API paths"
        );
    }
    app
}

/// /api/health — public liveness probe. No auth required. Shape is:
///
/// ```json
/// {
///   "status": "healthy",
///   "ok": true,
///   "service": "mailrs-webapi",
///   "version": "<pkg-version>",
///   "uptime_secs": 42,
///   "kevy": true,
///   "pg": null
/// }
/// ```
///
/// Callers can rely on:
///   - the four `service` / `version` / `uptime_secs` / `status` fields
///     always being present.
///   - `kevy` reporting the real ping status (this handler round-trips
///     a kevy op to compute the boolean, so it's not just a config
///     flag).
///   - `pg` being `null` in the fastcore lane (no PostgreSQL backend
///     exists), so the frontend can hide the PG pill instead of
///     drawing it as "down". In the spg-backed lane the same handler
///     ships with `pg` set to a real probe.
async fn health_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    // Cheap kevy round-trip. Any success => backend healthy; any error
    // => backend unreachable. Runs on the shared shard connection, no
    // fresh TCP per request.
    let kevy_ok = handlers::kevy_util::with_kevy(|c| c.ping()).is_ok();
    axum::Json(serde_json::json!({
        "status": if kevy_ok { "healthy" } else { "degraded" },
        "ok": kevy_ok,
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs,
        "kevy": kevy_ok,
        "pg": serde_json::Value::Null,
    }))
}

/// /api/readiness — deep probe: does core RPC answer?
async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    match state.core.readyz().await {
        Ok(h) if h.ready => Ok(axum::Json(serde_json::json!({
            "status": "ready",
            "core_version": h.version,
        }))),
        Ok(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// /api/status — version + build info + webapi lifetime. No auth
/// required. Additional metric fields (SMTP counters, queue depth) are
/// nulled out here rather than pretending they're zero: in the fastcore
/// 4-process split those counters live in `mailrs-receiver` +
/// `mailrs-fastcore-sender`, not in this webapi process. UIs that render
/// them treat `null` as "no data" (a dash), which is the truthful thing
/// to show — v1.9.4 shipped a monitor page that read the absent fields
/// as `0` and displayed `NaN` uptime; explicit nulls fix both.
async fn status_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "active_connections": serde_json::Value::Null,
        "total_connections": serde_json::Value::Null,
        "total_messages": serde_json::Value::Null,
        "queue": serde_json::Value::Null,
    }))
}

/// Phase 3 stub auth middleware — extracts user from `X-Mailrs-User`
/// header. Real session/JWT/api-key resolution lands in checklist 3.9.
async fn stub_auth_middleware(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let user = req
        .headers()
        .get("X-Mailrs-User")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    req.extensions_mut()
        .insert(handlers::conversations::AuthedDisplayName::default());
    req.extensions_mut()
        .insert(handlers::conversations::AuthedUser(user));
    Ok(next.run(req).await)
}

/// Main entry — boots state, builds router, listens, handles shutdown.
pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Prometheus recorder must be installed before any counter is
    // emitted; do it as early as possible in the boot sequence.
    handlers::metrics::install();

    let state = Arc::new(WebState::from_env());
    tracing::info!(
        bind = %state.bind_addr,
        version = env!("CARGO_PKG_VERSION"),
        "mailrs-webapi starting"
    );

    // Quick core liveness probe so we fail-fast on bad MAILRS_CORE_RPC_BASE.
    match state.core.healthz().await {
        Ok(h) => {
            tracing::info!(version = %h.version, backend = ?h.backend, "core RPC reachable");
        }
        Err(e) => {
            tracing::warn!(error = %e, "core RPC unreachable at startup — webapi will retry");
        }
    }

    // One-shot alias sync: existing alias entries in the network kevy
    // `admin:aliases` hash (populated by webapi's older `add_alias`
    // handler) don't have a fastcore mirror. Push each into fastcore
    // on boot so the spool drain sees them immediately. Idempotent.
    {
        let sync_state = state.clone();
        tokio::spawn(async move {
            let synced = crate::handlers::admin::sync_aliases_to_fastcore(&sync_state).await;
            if synced > 0 {
                tracing::info!(count = synced, "alias sync: network → fastcore");
            }
        });
    }

    let bind = state.bind_addr.clone();
    let router = build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|e| panic!("webapi bind {bind} failed: {e}"));
    tracing::info!(addr = %bind, "webapi listening");

    let (_shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        let _ = shutdown_rx.changed().await;
    });

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            r = tokio::signal::ctrl_c() => r.expect("ctrl_c"),
            _ = sigterm.recv() => {}
            r = server => { if let Err(e) = r { tracing::error!(error = %e, "webapi server exited"); } return; }
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.expect("ctrl_c");

    tracing::info!("mailrs-webapi shutting down");
    let _ = _shutdown_tx.send(true);
}
