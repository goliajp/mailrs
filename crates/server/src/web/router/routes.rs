//! Per-area route builders. Each fn returns an
//! `axum::Router<Arc<WebState>>` for one logical API surface.
//! Composed into the top-level router by `router::router()`.

use std::sync::Arc;

use axum::routing::{any, delete, get, post, put};

use super::super::{
    WebState, admin, ai_assist, api_key, auth, autodiscover, calendar_api, conversations, dav,
    jmap, mail, oidc_provider, rsvp, system_config, templates, webhook, ws,
};

pub(super) fn core_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // status + health
        .route("/api/status", get(admin::get_status))
        .route("/api/health", get(admin::get_health))
        .route("/api/readiness", get(admin::get_readiness))
        .route("/metrics", get(admin::prometheus_metrics))
        // websocket
        .route("/api/events", get(ws::ws_events))
        // queue
        .route("/api/queue", get(admin::get_queue))
        .route("/api/queue/{id}/retry", post(admin::retry_queue_message))
        // cache flush
        .route(
            "/api/admin/cache/flush-conversations",
            post(admin::flush_conversations),
        )
}
pub(super) fn mail_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // mail API
        .route("/api/calendar/conflicts", get(calendar_api::get_conflicts))
        .route(
            "/api/calendar/feeds",
            get(calendar_api::list_feeds).post(calendar_api::create_feed),
        )
        .route(
            "/api/calendar/feeds/{feed_id}",
            axum::routing::delete(calendar_api::delete_feed),
        )
        .route("/api/invites/{message_id}/rsvp", post(rsvp::submit_rsvp))
        .route(
            "/api/invites/{message_id}/counter",
            post(rsvp::submit_counter),
        )
        .route("/api/mail/folders", get(mail::get_folders))
        .route(
            "/api/mail/folders/{name}/messages",
            get(mail::get_folder_messages),
        )
        .route("/api/mail/messages/{uid}", get(mail::get_message))
        .route(
            "/api/mail/messages/{uid}/flags",
            post(mail::update_message_flags),
        )
        .route("/api/mail/messages/{uid}", delete(mail::delete_message))
        .route("/api/mail/export", get(mail::export_mbox))
        .route("/api/mail/send", post(mail::send_message))
        .route(
            "/api/mail/check-deliverability",
            post(mail::check_deliverability),
        )
        .route("/api/mail/spam-feedback", post(mail::submit_spam_feedback))
        .route("/api/mail/render-preview", post(mail::render_preview))
        .route(
            "/api/mail/render-preview/cache/{id}",
            get(mail::serve_render_cache),
        )
        .route(
            "/api/admin/spam-feedback-stats",
            get(mail::get_spam_feedback_stats),
        )
        .route(
            "/api/mail/send-multipart",
            post(mail::send_message_multipart),
        )
        .route(
            "/api/mail/pending/{message_id}",
            delete(mail::cancel_pending_send),
        )
        .route("/api/mail/messages/{uid}/raw", get(mail::get_message_raw))
        .route(
            "/api/mail/messages/{uid}/attachments/{index}",
            get(mail::get_attachment),
        )
        // attachment content (OCR/PDF text)
        .route(
            "/api/mail/messages/{uid}/attachments/{index}/content",
            get(mail::get_attachment_content),
        )
        // inline image upload/serve
        .route("/api/mail/inline-upload", post(mail::upload_inline_image))
        .route("/api/mail/inline/{id}", get(mail::serve_inline_image))
        // drafts API
        .route(
            "/api/mail/drafts",
            post(mail::save_draft).get(mail::list_drafts),
        )
        .route("/api/mail/drafts/{id}", delete(mail::delete_draft))
        // signatures API
        .route(
            "/api/mail/signatures",
            post(mail::save_signature).get(mail::list_signatures),
        )
        .route("/api/mail/signatures/{id}", delete(mail::delete_signature))
        // encryption keys API
        .route("/api/mail/keys", get(mail::list_keys))
        .route(
            "/api/mail/keys/{key_type}",
            get(mail::get_key)
                .put(mail::set_key)
                .delete(mail::delete_key),
        )
        // public key lookup (no auth required, rate-limited by general_rate_limit layer)
        .route("/api/keys/{address}/pgp", get(mail::get_public_pgp_key))
        .route("/api/keys/{address}/smime", get(mail::get_public_smime_key))
        // templates API
        .route(
            "/api/mail/templates",
            post(templates::save_template).get(templates::list_templates),
        )
        .route(
            "/api/mail/templates/{id}",
            delete(templates::delete_template),
        )
        // AI assist
        .route("/api/mail/ai/polish", post(ai_assist::ai_polish))
        .route(
            "/api/mail/ai/reply-suggest",
            post(ai_assist::ai_reply_suggest),
        )
        .route(
            "/api/mail/ai/generate-subject",
            post(ai_assist::ai_generate_subject),
        )
}
pub(super) fn conversations_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // conversations API
        .route("/api/conversations", get(conversations::get_conversations))
        .route(
            "/api/conversations/batch",
            post(conversations::batch_conversations),
        )
        .route(
            "/api/conversations/categories",
            get(conversations::get_conversation_categories),
        )
        .route(
            "/api/conversations/action-count",
            get(conversations::get_action_count),
        )
        .route(
            "/api/conversations/search",
            get(conversations::search_conversations),
        )
        .route(
            "/api/conversations/semantic-search",
            get(conversations::semantic_search),
        )
        .route(
            "/api/conversations/{thread_id}",
            get(conversations::get_thread_messages).delete(conversations::delete_thread),
        )
        .route(
            "/api/conversations/{thread_id}/read",
            post(conversations::mark_thread_read),
        )
        .route(
            "/api/conversations/{thread_id}/unread",
            post(conversations::mark_thread_unread),
        )
        .route(
            "/api/conversations/{thread_id}/star",
            post(conversations::star_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unstar",
            post(conversations::unstar_thread),
        )
        .route(
            "/api/conversations/{thread_id}/pin",
            post(conversations::pin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unpin",
            post(conversations::unpin_thread),
        )
        .route(
            "/api/conversations/{thread_id}/dismiss-action",
            post(conversations::dismiss_action),
        )
        .route(
            "/api/conversations/{thread_id}/archive",
            post(conversations::archive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/unarchive",
            post(conversations::unarchive_thread),
        )
        .route(
            "/api/conversations/{thread_id}/snooze",
            put(conversations::snooze_thread).delete(conversations::unsnooze_thread),
        )
        .route(
            "/api/conversations/{thread_id}/messages/{uid}/reactions",
            put(conversations::toggle_reaction),
        )
        .route(
            "/api/conversations/{thread_id}/reactions",
            get(conversations::get_thread_reactions),
        )
        .route("/api/contacts", get(conversations::get_contacts))
        .route("/api/mail/stats", get(conversations::get_mail_stats))
        .route("/api/mail/feedback", post(conversations::record_feedback))
}
pub(super) fn account_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // (auth login routes are merged separately in router() because
        //  they carry their own per-IP rate limiter built from runtime state)
        .route("/api/auth/logout", post(auth::logout))
        .route("/api/auth/me", get(auth::auth_me))
        // self-service password change
        .route("/api/auth/change-password", post(auth::change_password))
        // recovery email
        .route(
            "/api/auth/recovery-email",
            get(auth::get_recovery_email).post(auth::update_recovery_email),
        )
        // identity verification (for external IdPs)
        .route("/api/auth/verify", post(auth::verify_credentials))
        .route("/api/auth/verify-totp", post(auth::verify_totp))
        // OIDC client (Sign in with GOLIA)
        .route("/api/auth/oidc/login", get(auth::oidc_login))
        .route("/api/auth/oidc/callback", get(auth::oidc_callback))
        .route("/api/auth/oidc/config", get(auth::oidc_client_config))
        // TOTP 2FA
        .route("/api/auth/totp/setup", post(auth::totp_setup))
        .route("/api/auth/totp/enable", post(auth::totp_enable))
        .route("/api/auth/totp/disable", post(auth::totp_disable))
        .route("/api/auth/totp/status", get(auth::totp_status))
}
pub(super) fn agent_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // API key management
        .route(
            "/api/agent/keys",
            post(api_key::create_api_key).get(api_key::list_api_keys),
        )
        .route("/api/agent/keys/{id}", delete(api_key::revoke_api_key))
        // webhook subscriptions
        .route(
            "/api/agent/webhooks",
            post(webhook::create_webhook).get(webhook::list_webhooks),
        )
        .route("/api/agent/webhooks/{id}", delete(webhook::delete_webhook))
}
pub(super) fn admin_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // admin API
        .route(
            "/api/admin/domains",
            get(admin::list_domains).post(admin::add_domain),
        )
        .route("/api/admin/domains/{name}", delete(admin::remove_domain))
        .route(
            "/api/admin/domains/{name}/check",
            post(admin::check_domain_handler),
        )
        .route("/api/admin/rbl-status", get(admin::get_rbl_status))
        .route(
            "/api/admin/reconcile-maildir",
            post(admin::reconcile::reconcile_maildir),
        )
        .route("/api/admin/reputation", get(admin::get_reputation))
        .route("/api/admin/export", get(admin::export_messages))
        .route(
            "/api/admin/suppressions",
            get(admin::list_suppressed).delete(admin::remove_suppressed),
        )
        .route(
            "/api/admin/accounts",
            get(admin::list_accounts).post(admin::add_account),
        )
        .route(
            "/api/admin/accounts/{address}",
            put(admin::update_account).delete(admin::remove_account),
        )
        .route(
            "/api/admin/aliases",
            get(admin::list_aliases).post(admin::add_alias),
        )
        .route("/api/admin/aliases/{id}", delete(admin::remove_alias))
        // quota + sieve
        .route(
            "/api/admin/accounts/{address}/quota",
            get(admin::get_quota).post(admin::set_quota),
        )
        .route(
            "/api/admin/accounts/{address}/sieve",
            get(admin::get_sieve)
                .post(admin::set_sieve)
                .delete(admin::delete_sieve),
        )
        // groups CRUD
        .route(
            "/api/admin/groups",
            get(admin::list_groups).post(admin::create_group),
        )
        .route("/api/admin/groups/{id}", delete(admin::delete_group))
        .route(
            "/api/admin/groups/{id}/permissions",
            get(admin::get_group_permissions).put(admin::set_group_permissions),
        )
        .route(
            "/api/admin/groups/{id}/members",
            get(admin::list_group_members).post(admin::add_group_member),
        )
        .route(
            "/api/admin/groups/{id}/members/{address}",
            delete(admin::remove_group_member),
        )
        .route(
            "/api/admin/accounts/{address}/groups",
            get(admin::get_account_groups),
        )
        .route(
            "/api/admin/accounts/{address}/overrides",
            get(admin::get_account_overrides).put(admin::set_account_overrides),
        )
        .route("/api/admin/permissions", get(admin::get_all_permissions))
        // email groups
        .route(
            "/api/admin/email-groups",
            get(admin::list_email_groups).post(admin::create_email_group),
        )
        .route(
            "/api/admin/email-groups/{id}",
            delete(admin::delete_email_group),
        )
        .route(
            "/api/admin/email-groups/{id}/members",
            get(admin::list_email_group_members).post(admin::add_email_group_member),
        )
        .route(
            "/api/admin/email-groups/{id}/members/{address}",
            delete(admin::remove_email_group_member),
        )
        // apps
        .route(
            "/api/admin/apps",
            get(admin::list_apps).post(admin::create_app),
        )
        .route(
            "/api/admin/apps/{app_id}",
            get(admin::get_app).delete(admin::delete_app),
        )
        .route(
            "/api/admin/apps/{app_id}/scopes",
            put(admin::update_app_scopes),
        )
        // greylist local lists (Phase 2)
        .route(
            "/api/admin/greylist/local-lists",
            get(admin::greylist_local::list).post(admin::greylist_local::create),
        )
        .route(
            "/api/admin/greylist/local-lists/{id}",
            delete(admin::greylist_local::remove),
        )
        // audit log
        .route("/api/admin/audit-log", get(admin::get_audit_log))
        // mail audit (admin impersonate)
        .route("/api/admin/audit/accounts", get(admin::audit_list_accounts))
        .route(
            "/api/admin/audit/conversations",
            get(admin::audit_list_conversations),
        )
        .route(
            "/api/admin/audit/conversations/{thread_id}/messages",
            get(admin::audit_get_thread_messages),
        )
        .route(
            "/api/admin/audit/messages/{uid}/raw",
            get(admin::audit_get_raw_message),
        )
        // smtp config
        .route("/api/admin/config/smtp", get(admin::get_smtp_config))
        // system config (runtime-editable)
        .route("/api/admin/system-config", get(system_config::list_config))
        .route(
            "/api/admin/system-config/{key}",
            put(system_config::update_config).delete(system_config::reset_config),
        )
}
pub(super) fn protocol_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // JMAP
        .route("/.well-known/jmap", get(jmap::jmap_session))
        .route("/jmap", post(jmap::jmap_api))
        .route("/jmap/eventsource/", get(jmap::jmap_eventsource))
        // OIDC provider
        .route(
            "/.well-known/openid-configuration",
            get(oidc_provider::openid_configuration),
        )
        .route("/.well-known/jwks.json", get(oidc_provider::jwks))
        .route("/oauth/authorize", get(oidc_provider::authorize))
        .route("/oauth/token", post(oidc_provider::token))
        .route("/oauth/userinfo", get(oidc_provider::userinfo))
        // OAuth client admin
        .route(
            "/api/admin/oauth-clients",
            post(admin::create_oauth_client).get(admin::list_oauth_clients),
        )
        .route(
            "/api/admin/oauth-clients/{client_id}",
            delete(admin::delete_oauth_client),
        )
        // MTA-STS policy
        .route("/.well-known/mta-sts.txt", get(admin::mta_sts_policy))
        // mail client autodiscover
        .route(
            "/autodiscover/autodiscover.xml",
            post(autodiscover::autodiscover_outlook),
        )
        .route(
            "/Autodiscover/Autodiscover.xml",
            post(autodiscover::autodiscover_outlook),
        )
        .route(
            "/.well-known/autoconfig/mail/config-v1.1.xml",
            get(autodiscover::autoconfig_mozilla),
        )
        .route(
            "/mail/config-v1.1.xml",
            get(autodiscover::autoconfig_mozilla),
        )
}
pub(super) fn dav_routes() -> axum::Router<Arc<WebState>> {
    axum::Router::new()
        // CalDAV / CardDAV (well-known redirects + DAV endpoints)
        .route("/.well-known/caldav", any(dav::well_known_caldav))
        .route("/.well-known/carddav", any(dav::well_known_carddav))
        .route("/dav/", any(dav::dav_principal))
        .route("/dav/calendars/{user}/", any(dav::dav_calendar_home))
        .route(
            "/dav/calendars/{user}/{calendar}/",
            any(dav::dav_calendar_collection),
        )
        .route(
            "/dav/calendars/{user}/{calendar}/{uid}",
            any(dav::dav_event),
        )
        .route("/dav/contacts/{user}/", any(dav::dav_contact_home))
        .route(
            "/dav/contacts/{user}/{book}/",
            any(dav::dav_contact_collection),
        )
        .route("/dav/contacts/{user}/{book}/{uid}", any(dav::dav_contact))
}
