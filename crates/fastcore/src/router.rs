//! The route table.
//!
//! Split out of `lib.rs` on 2026-08-02 — 479 lines of `.route(...)` in the
//! middle of the file that also held the handlers, the sweep and the
//! ingest loop.
//!
//! Its own test is the point: a route string that fails to parse panics at
//! construction, which is process start, in production. 4,483 workspace
//! tests were green while not one of them built this table, and 2.19.0
//! went out with the REST API in a restart loop.

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};

use crate::*;

pub fn build_router(state: Arc<FastcoreState>) -> Router {
    let base = base_router(state.clone());
    // One Router for all business routes so matchit's trie sees the
    // full set at once. Earlier split into convo + thread Routers
    // hit a route-resolution bug where only the first-registered
    // route under /v1/users/{user}/conversations matched at runtime —
    // probable matchit collision between `conversations:list` (literal
    // ":list") and `conversations/categories` (path-separator). A
    // single Router with all routes registered side-by-side resolves it.
    let business =
        Router::new()
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
            .route(
                th::PATH_FIND_THREAD_BY_MESSAGE_ID,
                get(find_thread_by_message_id),
            )
            .route(th::PATH_BACKFILL_THREADING, post(backfill_threading_route))
            .route(
                "/v1/admin/backfill-decode-headers",
                post(backfill_decode::backfill_decode_headers_route),
            )
            .route("/v1/admin/threads:split-message", post(split_message_route))
            .route("/v1/admin/maintenance:rewrite-aof", post(rewrite_aof_route))
            .route(th::PATH_DELIVER_MESSAGE, post(deliver_message))
            .route(th::PATH_MARK_READ, post(mark_read))
            .route(th::PATH_MARK_ALL_READ, post(mark_all_read_route))
            .route(th::PATH_MARK_LIST_READ, post(mark_list_read_route))
            .route(th::PATH_MARK_UNREAD, post(mark_unread_route))
            .route(th::PATH_SNOOZE, put(snooze_thread_route))
            .route(th::PATH_UNSNOOZE, delete(unsnooze_thread_route))
            .route(th::PATH_PIN, post(pin_thread))
            .route(th::PATH_UNPIN, post(unpin_thread))
            .route(th::PATH_STAR, post(star_thread))
            .route(th::PATH_UNSTAR, post(unstar_thread))
            .route(th::PATH_ARCHIVE, post(archive_thread))
            .route(th::PATH_UNARCHIVE, post(unarchive_thread))
            .route(th::PATH_MARK_JUNK, post(mark_junk))
            .route(th::PATH_MARK_NOT_JUNK, post(mark_not_junk))
            .route(th::PATH_MARK_NOTIFICATION, post(mark_notification))
            .route(th::PATH_MARK_PROMOTION, post(mark_promotion))
            .route(th::PATH_MOVE_TO_INBOX, post(move_to_inbox))
            .route(th::PATH_DELETE_THREAD, delete(delete_thread))
            .route(adm::PATH_GET_ACCOUNT_HASH, get(get_account_with_hash))
            .route(adm::PATH_EFFECTIVE_PERMISSIONS, get(effective_permissions))
            .route(
                adm::PATH_LIST_ACCOUNTS,
                get(list_accounts).post(add_account_route),
            )
            .route(
                adm::PATH_UPDATE_ACCOUNT,
                put(update_account_route).delete(remove_account_route),
            )
            .route(adm::PATH_SET_QUOTA, post(set_quota_route))
            .route(
                adm::PATH_UPDATE_RECOVERY_EMAIL,
                post(set_recovery_email_route),
            )
            .route(adm::PATH_SET_ACCOUNT_PASSWORD, post(set_password_route))
            .route(adm::PATH_SET_MESSAGE_FLAGS, post(set_message_flags_route))
            // Aliases live in the fastcore-embedded kevy so the spool drain
            // (also in-process) can resolve `contact@golia.jp -> lihao` and
            // similar single-hop forwards. Distinct namespace from webapi's
            // network-kevy `admin:aliases` hash — that older store is not
            // consulted by the drain and stays around only until UI wiring
            // catches up.
            .route(
                "/v1/admin/aliases:local",
                get(list_local_aliases).post(upsert_local_alias),
            )
            .route(
                "/v1/admin/aliases:local/{source}",
                delete(delete_local_alias_route),
            )
            // Ops endpoint — reset every user's ingest cursor to 0 so the
            // next sync tick re-processes historic threads and (via the
            // Group F diff path) backfills messages fastcore missed under
            // the older "skip-existing" ingest behaviour.
            .route(
                "/v1/admin/sync/reset-cursors",
                post(reset_sync_cursors_route),
            )
            // Ops endpoint — one-shot pre-P6 legacy keyspace sweep
            // (Phase 11.2 embedded half). In-process so no AOF
            // double-open OOM; idempotent.
            .route(
                "/v1/admin/maintenance:sweep-legacy-admin-keys",
                post(sweep_legacy_admin_keys_route),
            )
            // Ops endpoint — give pre-existing webhook subscriptions the
            // owner entry their delete path now needs. Idempotent.
            .route(
                "/v1/admin/maintenance:backfill-webhook-owners",
                post(backfill_webhook_owners_route),
            )
            // Ops endpoint — fold the retired `agent:webhooks:{user}`
            // namespace into the one both surfaces now read. Idempotent.
            .route(
                "/v1/admin/maintenance:migrate-agent-webhooks",
                post(migrate_agent_webhooks_route),
            )
            // Stage 2 of the per-user message projection: give every user
            // their own row for the messages they actually have.
            .route(
                "/v1/admin/maintenance:backfill-user-messages",
                post(backfill_user_messages_route),
            )
            // One-shot: the per-user message index's first key spelling sat
            // inside the prefix `all_thread_ids_for_user` enumerates.
            .route(
                "/v1/admin/maintenance:drop-stray-usermsg-keys",
                post(drop_stray_usermsg_keys_route),
            )
            // Stage 3: compare the shared index against the per-user one
            // before anything reads the latter.
            .route(
                "/v1/admin/maintenance:threadrow-shadow",
                post(threadrow_shadow_route),
            )
            .route(
                "/v1/admin/maintenance:strip-shared-per-user-fields",
                post(strip_shared_per_user_fields_route),
            )
            .route(
                "/v1/admin/maintenance:usermsg-shadow",
                post(usermsg_shadow_route),
            )
            // Ops endpoint — where mail forging one of our own domains
            // actually ended up.
            .route(
                "/v1/admin/maintenance:spoof-landing",
                post(spoof_landing_route),
            )
            // Ops endpoint — remove thread rows that open onto nothing.
            .route(
                "/v1/admin/maintenance:drop-empty-threads",
                post(drop_empty_threads_route),
            )
            // Ops endpoint — seed the Bayesian corpus from existing
            // junk (spam) + inbox (ham) folders. One-shot; refuses if
            // the corpus is already non-empty.
            .route(
                "/v1/admin/maintenance:bayes-bootstrap",
                post(bayes_bootstrap_route),
            )
            // Ops endpoint — seed the v2.9 multi-class triage corpus +
            // re-sort existing Inbox mail into N/P (idempotent).
            .route(
                "/v1/admin/maintenance:backfill-triage",
                post(backfill_triage_route),
            )
            // Segmented promotion of existing threads into the
            // `threaduser` table's membership rows (v4 TABLE migration).
            // Paged on purpose: a full scan competes with live traffic
            // for the same store, so the caller drives it in batches.
            .route(
                "/v1/admin/maintenance:backfill-thread-user",
                post(backfill_thread_user_route),
            )
            // Rebuild thread counters from the messages they summarise.
            // The arrival path increments them by hand next to an index
            // that dedupes, so a message delivered to two local
            // mailboxes counts twice.
            .route(
                "/v1/admin/maintenance:recount-threads",
                post(recount_threads_route),
            )
            // The same two copies, compared rather than repaired. The
            // gate for reading from the per-user one.
            .route(
                "/v1/admin/maintenance:shadow-counts",
                post(shadow_counts_route),
            )
            .route(
                "/v1/admin/maintenance:sent-axis-shadow",
                post(sent_axis_shadow_route),
            )
            .route(
                "/v1/admin/maintenance:legacy-zset-census",
                post(legacy_zset_census_route),
            )
            // Engine-side reconciliation for the declared table: drift
            // recheck per compiled index plus a column-type spot check.
            .route(
                "/v1/admin/maintenance:table-verify",
                post(table_verify_route),
            )
            // Row-level census behind the VERIFY counters — answers
            // "which rows are missing from an index", which VERIFY
            // reports as a count and not an identity.
            .route(
                "/v1/admin/maintenance:threaduser-census",
                post(threaduser_census_route),
            )
            // Deletes the legacy per-user thread zsets. Nothing writes
            // or reads them any more; this reclaims the memory.
            .route(
                "/v1/admin/maintenance:drop-legacy-zsets",
                post(drop_legacy_zsets_route),
            )
            // RAM versus disk, so tiering can be judged on numbers.
            .route("/v1/admin/maintenance:tier-info", post(tier_info_route))
            // Shadow read — the engine's answer against the
            // hand-maintained zset's, before any read is cut over.
            .route("/v1/admin/maintenance:shadow-read", post(shadow_read_route))
            // Contact relationship counters, rebuilt from message
            // history so importance scoring sees existing correspondents
            // instead of waiting months for new traffic (idempotent).
            .route(
                "/v1/admin/maintenance:backfill-contact-relationships",
                post(backfill_contact_relationships_route),
            )
            // Importance verdicts for threads that predate the feature —
            // scoring only runs at ingest, so without this every existing
            // thread would stay blank forever.
            .route(
                "/v1/admin/maintenance:backfill-thread-importance",
                post(backfill_thread_importance_route),
            )
            .route(mb::PATH_LIST_MAILBOXES, get(list_mailboxes))
            .route(
                msg::PATH_GET_MESSAGE_BY_UID_USER,
                get(get_message_by_uid_for_user),
            )
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
                get(mailrs_core_sidestate::families::prefs::list_signatures::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::prefs::save_signature::<FastcoreState>),
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
            // reactions / webhooks / audit (network kevy)
            .route(
                adm::PATH_GET_THREAD_REACTIONS,
                get(
                    mailrs_core_sidestate::families::admin_state::get_thread_reactions::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_TOGGLE_REACTION,
                put(mailrs_core_sidestate::families::admin_state::toggle_reaction::<FastcoreState>),
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
                delete(
                    mailrs_core_sidestate::families::admin_state::delete_webhook::<FastcoreState>,
                ),
            )
            .route(
                adm::PATH_LIST_AUDIT_LOG,
                get(mailrs_core_sidestate::families::admin_state::list_audit_log::<FastcoreState>)
                    .post(mailrs_core_sidestate::families::admin_state::log_audit::<FastcoreState>),
            )
            // account / alias / domain — switchable mail store (embedded kevy)
            .route(adm::PATH_GET_ACCOUNT, get(routes::mail_admin::get_account))
            .route(
                adm::PATH_LIST_ALIASES,
                get(routes::mail_admin::list_aliases).post(routes::mail_admin::add_alias),
            )
            .route(
                adm::PATH_REMOVE_ALIAS,
                delete(routes::mail_admin::remove_alias),
            )
            .route(
                adm::PATH_LIST_DOMAINS,
                get(routes::mail_admin::list_domains).post(routes::mail_admin::add_domain),
            )
            .route(
                adm::PATH_REMOVE_DOMAIN,
                delete(routes::mail_admin::remove_domain),
            )
            // contacts — shared derived side-state (network kevy)
            .route(
                ct::PATH_SEARCH_CONTACTS,
                get(mailrs_core_sidestate::families::contacts::search_contacts::<FastcoreState>),
            )
            .route(
                ct::PATH_UPSERT_INBOUND,
                post(mailrs_core_sidestate::families::contacts::upsert_inbound::<FastcoreState>),
            )
            .route(
                ct::PATH_CONTACT_SCORING,
                get(mailrs_core_sidestate::families::contacts::contact_scoring::<FastcoreState>),
            )
            .route(
                ct::PATH_HAS_SENT_TO,
                get(mailrs_core_sidestate::families::contacts::has_sent_to::<FastcoreState>),
            )
            .route(
                ct::PATH_SENDER_FEEDBACK,
                post(mailrs_core_sidestate::families::contacts::sender_feedback::<FastcoreState>),
            )
            // analysis — shared derived side-state (network kevy); semantic 501
            .route(
                an::PATH_GET_ANALYSIS,
                get(mailrs_core_sidestate::families::analysis::get_analysis::<FastcoreState>),
            )
            .route(
                an::PATH_COUNT_UNANALYZED,
                get(mailrs_core_sidestate::families::analysis::count_unanalyzed::<FastcoreState>),
            )
            .route(
                an::PATH_BOOST_IMPORTANCE,
                post(mailrs_core_sidestate::families::analysis::boost_importance::<FastcoreState>),
            )
            .route(
                an::PATH_ATTACHMENT_TEXTS,
                get(mailrs_core_sidestate::families::analysis::attachment_texts::<FastcoreState>),
            )
            .route(
                an::PATH_SEMANTIC_SEARCH,
                post(mailrs_core_sidestate::families::analysis::semantic_search),
            )
            // outbound queue — shared network kevy (same keys the sender drains)
            .route(
                ob::PATH_ENQUEUE,
                post(mailrs_core_sidestate::families::outbound::enqueue::<FastcoreState>),
            )
            .route(
                ob::PATH_CLAIM,
                post(mailrs_core_sidestate::families::outbound::claim::<FastcoreState>),
            )
            .route(
                ob::PATH_STATS,
                get(mailrs_core_sidestate::families::outbound::stats::<FastcoreState>),
            )
            .route(
                ob::PATH_RECOVER_STALE,
                post(mailrs_core_sidestate::families::outbound::recover_stale::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_DELIVERED,
                post(mailrs_core_sidestate::families::outbound::mark_delivered::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_FAILED,
                post(mailrs_core_sidestate::families::outbound::mark_failed::<FastcoreState>),
            )
            .route(
                ob::PATH_MARK_BOUNCED,
                post(mailrs_core_sidestate::families::outbound::mark_bounced::<FastcoreState>),
            )
            // groups / permissions / api-keys / sieve (network kevy)
            .route(
                adm::PATH_LIST_GROUPS,
                get(mailrs_core_sidestate::families::groups_admin::list_groups::<FastcoreState>),
            )
            .route(
                adm::PATH_GET_GROUP_PERMISSIONS,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_group_permissions::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_LIST_GROUP_MEMBERS,
                get(
                    mailrs_core_sidestate::families::groups_admin::list_group_members::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_GET_ACCOUNT_GROUPS,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_account_groups::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_REMOVE_ACCOUNT_FROM_GROUP,
                delete(
                    mailrs_core_sidestate::families::groups_admin::remove_account_from_group::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_GET_API_KEY_BY_PREFIX,
                get(
                    mailrs_core_sidestate::families::groups_admin::get_api_key_by_prefix::<
                        FastcoreState,
                    >,
                ),
            )
            .route(
                adm::PATH_TOUCH_API_KEY,
                post(mailrs_core_sidestate::families::groups_admin::touch_api_key::<FastcoreState>),
            )
            .route(
                adm::PATH_GET_SIEVE,
                get(mailrs_core_sidestate::families::groups_admin::get_sieve::<FastcoreState>),
            )
            // mailbox CRUD — reuse the maildir IMAP backend
            .route(mb::PATH_GET_MAILBOX, get(routes::mailbox::get_mailbox))
            .route(
                mb::PATH_GET_MAILBOX_BY_ID,
                get(routes::mailbox::get_mailbox_by_id),
            )
            .route(
                mb::PATH_CREATE_MAILBOX,
                post(routes::mailbox::create_mailbox),
            )
            .route(
                mb::PATH_DELETE_MAILBOX,
                delete(routes::mailbox::delete_mailbox),
            )
            .route(
                mb::PATH_RENAME_MAILBOX,
                post(routes::mailbox::rename_mailbox),
            )
            .route(
                mb::PATH_MAILBOX_STATUS,
                get(routes::mailbox::mailbox_status),
            )
            // message ops — thread-store reads/flags + maildir copy/move/expunge
            .route(
                msg::PATH_GET_MESSAGE_BY_UID,
                get(routes::message::get_message_by_uid),
            )
            .route(
                msg::PATH_FIND_BY_MESSAGE_ID,
                get(routes::message::find_by_message_id),
            )
            .route(msg::PATH_LIST_MESSAGES, get(routes::message::list_messages))
            .route(msg::PATH_CHANGED_SINCE, get(routes::message::changed_since))
            .route(msg::PATH_SET_FLAGS, put(routes::message::set_flags))
            .route(
                msg::PATH_FLAGS_IF_UNCHANGED,
                post(routes::message::flags_if_unchanged),
            )
            .route(msg::PATH_COPY_MESSAGE, post(routes::message::copy_message))
            .route(msg::PATH_MOVE_MESSAGE, post(routes::message::move_message))
            .route(msg::PATH_EXPUNGE, post(routes::message::expunge))
            .with_state(state);

    base.merge(business)
}
