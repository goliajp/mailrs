//! Auth, JMAP, DAV, admin, and the unauthenticated surface.
//!
//! Split out of the 789-line `build_router` on 2026-08-02. The table is
//! built at process start, so a path string that fails to parse panics
//! there rather than on the first request.

use std::sync::Arc;

use crate::*;

/// Session-scoped auth: the routes that need a session to already exist.
pub(super) fn auth_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/api/auth/me", get(handlers::auth::auth_me))
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route(
            "/api/auth/change-password",
            post(handlers::auth::change_password),
        )
        .route("/api/auth/verify", post(handlers::auth::verify_credentials))
        .route("/api/auth/verify-totp", post(handlers::auth::verify_totp))
        // OIDC provider consent screen. Auth-required and correctly so:
        // `authorize` takes an `AuthedUser`, because consenting on behalf
        // of an account requires being signed in to it.
        //
        // The three sign-*in* starts that used to sit here do not, and are
        // now next to the callback in the unauthenticated router — see the
        // note there.
        .route("/oauth/authorize", get(handlers::oidc::authorize))
}

/// JMAP.
pub(super) fn jmap_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/.well-known/jmap", get(handlers::jmap::jmap_session))
        .route("/jmap", post(handlers::jmap::jmap_api))
        .route("/jmap/eventsource/", get(handlers::jmap::jmap_eventsource))
}

/// CalDAV and CardDAV.
pub(super) fn dav_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{any, put};

    axum::Router::new()
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
        )
}

/// The admin surface, behind `admin_middleware`.
pub(super) fn admin_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{delete, get, post, put};

    axum::Router::new()
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
        .route(
            "/api/admin/dmarc/reports",
            get(handlers::dmarc::list_reports),
        )
        .route(
            "/api/admin/dmarc/reports/{sid}",
            get(handlers::dmarc::get_report),
        )
        .route(
            "/api/admin/dmarc/sources",
            get(handlers::dmarc::list_sources),
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
        )
}

/// Reachable without a session: health, login, the OIDC endpoints and
/// the external-login callback. `login` sits outside `session_auth`
/// because a freshly-arrived client has no session to check.
pub(super) fn unauth_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{get, post};

    axum::Router::new()
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
            // The second path Thunderbird probes, at
            // `autoconfig.<domain>/mail/config-v1.1.xml`. The monolith
            // served both; this lane served only the well-known one, so
            // auto-setup failed for anyone whose client tried the
            // subdomain first.
            "/mail/config-v1.1.xml",
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
        // External IdP sign-in. Every step is unauthenticated, because
        // signing in is what happens before there is a session.
        //
        // Until 2026-08-01 the first three sat in `auth_routes` and
        // answered 401, so the flow could not begin: the login page asks
        // `external-providers` which buttons to draw and got 401, whose
        // empty body the page's `.catch` made indistinguishable from "no
        // provider is configured" — so no button was ever drawn, and the
        // callback below, correctly public all along, was unreachable.
        // `list_providers` even documents itself as "unauthenticated on
        // purpose"; the wiring simply disagreed with it.
        .route(
            "/api/auth/external-providers",
            get(handlers::external_login::list_providers),
        )
        .route(
            "/api/auth/external/{provider}",
            get(handlers::external_login::start),
        )
        .route("/api/auth/oidc/login", get(handlers::oidc::oidc_login))
        .route(
            // The relying-party callback. Was a stub that rendered the
            // authorization code into an HTML page and stopped.
            "/api/auth/oidc/callback",
            get(handlers::external_login::callback),
        )
        // DAV well-known redirects (unauth — DAV spec allows anonymous discovery).
        .route("/.well-known/caldav", get(handlers::dav::well_known_caldav))
        .route(
            "/.well-known/carddav",
            get(handlers::dav::well_known_carddav),
        )
}
