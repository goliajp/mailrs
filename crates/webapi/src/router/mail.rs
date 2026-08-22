//! Conversation and mail routes.
//!
//! Split out of the 789-line `build_router` on 2026-08-02. The table is
//! built at process start, so a path string that fails to parse panics
//! there rather than on the first request.

use crate::*;
/// Conversation list, threads, and the per-thread verbs.
use std::sync::Arc;

pub(super) fn conversation_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use crate::handlers::conversations as c;
    use axum::routing::{get, post, put};

    axum::Router::new()
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
        )
}

/// Everything under `/api/mail` — send, drafts, signatures, search,
/// attachments, the AI assists, and the per-user settings.
pub(super) fn mail_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers;
    use axum::routing::{delete, get, post, put};

    axum::Router::new()
        .route(
            "/api/auth/identities",
            get(handlers::external_login::list_identities),
        )
        .route(
            "/api/auth/identities:unlink",
            post(handlers::external_login::unlink_identity),
        )
        .route("/api/mail/folders", get(handlers::mail::get_folders))
        // Writing assistance. Registered whether or not a model is
        // configured: an unconfigured route that says so beats a missing one
        // that answers 405.
        .route("/api/mail/ai/polish", post(handlers::ai::ai_polish))
        .route(
            "/api/mail/ai/reply-suggest",
            post(handlers::ai::ai_reply_suggest),
        )
        .route(
            "/api/mail/ai/generate-subject",
            post(handlers::ai::ai_generate_subject),
        )
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
        .route(
            "/api/mail/unsubscribe",
            post(handlers::unsubscribe::unsubscribe),
        )
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
        // The Send list — one row per send, with delivery status. Not
        // wired into the UI yet; `:shadow` is the gate that says whether
        // it is safe to (RFC 20260730-send-status S3).
        .route("/api/mail/sends", get(handlers::sends::list_sends))
        .route(
            "/api/mail/sends:shadow",
            post(handlers::sends::shadow_sends),
        )
        // A path segment, not the `:verb` suffix the collection-level routes
        // use. matchit rejects a parameter with a literal suffix in the same
        // segment — `{send_id}:source` panics the router at construction
        // with "Only one parameter is allowed per path segment", which took
        // webapi-fc into a restart loop on 2.19.0. `sends:shadow` works
        // because that segment is pure literal, with no parameter in it.
        //
        // The stored RFC 5322 bytes — for download or inspection, and what
        // resend re-enqueues unchanged.
        .route(
            "/api/mail/sends/{send_id}/source",
            get(handlers::sends::send_source),
        )
        .route(
            "/api/mail/sends/{send_id}/resend",
            post(handlers::sends::resend),
        )
        // Re-edit: compose fields plus attachment *metadata*. The bytes
        // stay server-side and the following send names the ones to keep
        // by index (RFC 20260730-send-status S4 addendum).
        .route(
            "/api/mail/sends/{send_id}/redraft",
            get(handlers::sends::send_redraft),
        )
        .route(
            "/api/push/tokens",
            post(handlers::push::register_push_token),
        )
        .route(
            "/api/push/tokens/{token}",
            delete(handlers::push::delete_push_token),
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
        // Mailboxes somewhere else — see
        // .claude/rfcs/20260823-external-accounts.md
        .route(
            "/api/accounts/external",
            get(handlers::external_accounts::list).post(handlers::external_accounts::create),
        )
        .route(
            "/api/accounts/external/{id}",
            delete(handlers::external_accounts::delete),
        )
        // Gmail and Outlook refuse passwords for mail clients, so
        // this is the only way to connect one at all.
        .route(
            "/api/accounts/external/oauth/callback",
            get(handlers::account_oauth::callback),
        )
        .route(
            "/api/accounts/external/oauth/{provider}",
            get(handlers::account_oauth::start),
        )
        .route(
            "/api/accounts/external/settings",
            get(handlers::external_accounts::settings_for),
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
        .route("/api/scheduled", get(handlers::scheduled::list_scheduled))
        .route(
            "/api/scheduled/{id}/cancel",
            post(handlers::scheduled::cancel_scheduled),
        )
        .route(
            "/api/scheduled/{id}/reschedule",
            post(handlers::scheduled::reschedule_scheduled),
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
            "/api/agent/keys:migrate-legacy",
            post(handlers::complete::migrate_legacy_agent_key_indexes),
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
            // PUT and DELETE are what the admin page sends and what the
            // monolith serves. This lane had POST only, so editing or
            // resetting a setting answered 405.
            "/api/admin/system-config/{key}",
            put(handlers::complete::set_system_config_key)
                .post(handlers::complete::set_system_config_key)
                .delete(handlers::complete::delete_system_config_key),
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
            // Same story: the page saves with PUT, this lane took POST
            // only, so a group's permissions could never be saved.
            "/api/admin/groups/{id}/permissions",
            get(handlers::complete::get_group_permissions)
                .put(handlers::complete::set_group_permissions)
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
        )
}
