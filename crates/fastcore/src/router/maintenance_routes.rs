// CARVE-OUT: pure route registration; the builder chain is the table
//! The maintenance sweeps, kept out of the main route table.
//!
//! The routes that rebuild, audit or reclaim derived state. They run on
//! the maintenance runtime (`maintenance::offload`): most walk the corpus
//! synchronously, and on the serving runtime that stalls every request.
//! They are not part of what a client calls — each is run by hand, once,
//! when something needs repairing or measuring — so they are the
//! natural half to lift out when the router crosses the file-size
//! limit. `mod.rs` keeps the surface a client actually uses.
//!
//! Registration order does not matter to axum (paths are matched by a
//! trie, not in sequence) but a duplicate panics at construction, so
//! `tests/router_builds.rs` builds the whole table.

use super::*;

pub(super) fn maintenance_routes(r: Router<Arc<FastcoreState>>) -> Router<Arc<FastcoreState>> {
    let walks = Router::new()
        .route("/v1/admin/maintenance:rewrite-aof", post(rewrite_aof_route))
        .route(th::PATH_BACKFILL_THREADING, post(backfill_threading_route))
        .route(
            "/v1/admin/backfill-decode-headers",
            post(backfill_decode::backfill_decode_headers_route),
        )
        // Usage against the default quota limit; `?dry_run=false` gives the
        // default to accounts created without one.
        .route(adm::PATH_QUOTA_LIMITS, post(quota_limits_route))
        // Deleted accounts whose retention has passed, counted per store;
        // `?dry_run=false` removes them for good, one audit row each.
        .route(
            adm::PATH_ACCOUNT_PURGE,
            post(crate::account_purge::account_purge_route),
        )
        // Deleted email groups whose retention has passed, their team
        // state counted; `?dry_run=false` removes it, one audit row each.
        .route(
            adm::PATH_GROUP_PURGE,
            post(crate::account_purge::group_purge_route),
        )
        // Read-only. Answers "is a query running with no declared path" and
        // "is a declared path carrying no queries" — neither of which this
        // store could answer before kevy 5.1.
        .route(
            "/v1/admin/maintenance:idx-advice",
            post(crate::maintenance::idx_advice_route),
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
            "/v1/admin/maintenance:future-dates",
            post(crate::maintenance::future_dates::future_dates_route),
        )
        .route(
            "/v1/admin/maintenance:thread-date-audit",
            post(crate::maintenance::thread_date_audit::thread_date_audit_route),
        )
        .route(
            "/v1/admin/maintenance:strip-shared-per-user-fields",
            post(strip_shared_per_user_fields_route),
        )
        .route(
            "/v1/admin/maintenance:usermsg-shadow",
            post(usermsg_shadow_route),
        )
        // Read-only: the maildir's flags against the index's belief about
        // them, run before anything writes either side.
        .route(
            "/v1/admin/maintenance:read-state-shadow",
            post(read_state_shadow_route),
        )
        // Writes. `?dry_run=true` reports without changing. A second run
        // must report `changed: 0`.
        .route(
            "/v1/admin/maintenance:read-state-backfill",
            post(read_state_backfill_route),
        )
        // Gives back the file reference to rows the read path returns in
        // full with an empty `blob_ref` — listed in the mailbox, body
        // unfetchable. `?dry_run=true` reports first.
        .route(
            "/v1/admin/maintenance:recompute-sender-trust",
            post(crate::maintenance::recompute_sender_trust_route),
        )
        .route(
            "/v1/admin/maintenance:fraud-rescan",
            post(crate::maintenance::fraud_rescan_route),
        )
        // Read-only: the screening rules over stored mail, for the
        // review before they act. The body is the bundle under review.
        .route(
            "/v1/admin/maintenance:screening-replay",
            post(crate::maintenance::screening_replay_route),
        )
        // Read-only: what the fact ledger would take in memory, measured
        // by writing its entries for real mail into a throwaway store.
        // Read-only: the audit chain recomputed, and compared with its
        // replica, now rather than at the daily check.
        .route(
            "/v1/admin/maintenance:audit-verify",
            post(mailrs_core_sidestate::families::audit_chain::verify_route::<FastcoreState>),
        )
        // Read-only: what the shadow re-judge has done and would have done.
        .route(
            "/v1/admin/maintenance:rejudge-status",
            get(crate::rejudge::status_route),
        )
        // One shadow re-judge round now: records verdicts, moves nothing.
        .route(
            "/v1/admin/maintenance:rejudge-now",
            post(crate::rejudge::now_route),
        )
        .route(
            "/v1/admin/maintenance:ledger-estimate",
            post(crate::maintenance::ledger_estimate_route),
        )
        // Read-only: which stored Sieve scripts the forwarding rules change.
        .route(
            "/v1/admin/maintenance:sieve-survey",
            get(crate::maintenance::sieve_survey_route),
        )
        // Each account's usage counter against its maildir: GET reports,
        // POST `?dry_run=false` corrects.
        .route(
            adm::PATH_USAGE_DRIFT,
            get(crate::maintenance::usage_drift_route)
                .post(crate::maintenance::usage_drift_apply_route),
        )
        // Read-only: what the report fingerprints and their index hold.
        .route(
            "/v1/admin/maintenance:template-footprint",
            get(crate::report::footprint),
        )
        // Reads the calendar part out of mail that arrived before
        // anybody looked at one — which is all of it, since production
        // ingested invitations for a year and stored nothing about
        // them. `?dry_run=true` reports first.
        .route(
            "/v1/admin/maintenance:backfill-invites",
            post(crate::maintenance::backfill_invites_route),
        )
        .route(
            "/v1/admin/maintenance:repair-blob-refs",
            post(repair_blob_refs_route),
        )
        // Every file each copy of a message already has, so a delete takes
        // all of them. `?dry_run=true` counts first; a second run adds 0.
        .route(
            "/v1/admin/maintenance:backfill-message-files",
            post(crate::maintenance::backfill_message_files_route),
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
        // The rebuild the three sidecar files exist for: put the
        // maildir's own facts back onto every thread row, not just the
        // ones the sweep happens to create.
        .route(
            "/v1/admin/maintenance:reindex",
            post(crate::maintenance::reindex_route),
        )
        // One-time bridge: write each mailbox's uidlist from the UIDs its
        // index already holds. Without it the file only ever describes
        // mail that arrived after the deploy.
        .route(
            "/v1/admin/maintenance:uidlist-backfill",
            post(crate::maintenance::uidlist_backfill_route),
        )
        // A message with no UID cannot be fetched by one — the raw view
        // and every attachment download go through it — and the web uses
        // the UID as the timeline's React key.
        .route(
            "/v1/admin/maintenance:allocate-missing-uids",
            post(crate::maintenance::allocate_missing_uids_route),
        )
        // Housekeeping for the append-only uidlist: one record per
        // message, in UID order. Reports what it walked as well as what it
        // dropped, so a run with nothing to do says so.
        .route(
            "/v1/admin/maintenance:uidlist-compact",
            post(crate::maintenance::uidlist_compact_route),
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
        // Retrain the v2 spam/ham corpus (decoded-text tokenizer) from the
        // thread ids in the legacy corpus sets. Paged with skip/limit, dry
        // by default.
        .route(
            "/v1/admin/maintenance:bayes-rebuild-v2",
            post(crate::maintenance::bayes_rebuild_v2_route),
        )
        // One-off: authenticated newsletters the old score put in Junk go
        // to the bucket triage gives them. Dry by default.
        .route(
            "/v1/admin/maintenance:junk-bulk-correct",
            post(crate::maintenance::junk_bulk_correct_route),
        )
        // Segmented promotion of existing threads into the
        // `threaduser` table's membership rows (v4 TABLE migration).
        // Paged on purpose: a full scan competes with live traffic
        // for the same store, so the caller drives it in batches.
        .route(
            "/v1/admin/maintenance:backfill-domain-seen",
            post(crate::maintenance::backfill_domain_seen_route),
        )
        // Records which sending domains funnel into each off-domain
        // reply address, so the rotation rule can convict a campaign
        // whose first three domains already delivered.
        // One file on disk, two message rows: the monolith registered
        // a message under its real Message-ID and something later
        // registered the same file under the synthetic id that path
        // mints when there is none. Read-only — deleting a row needs
        // to know which UID a client already has.
        .route(
            "/v1/admin/maintenance:duplicate-blob-census",
            post(crate::maintenance::duplicate_blob_census_route),
        )
        .route(
            "/v1/admin/maintenance:duplicate-blob-repair",
            post(crate::maintenance::duplicate_blob_repair_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-reply-rotation",
            post(crate::maintenance::backfill_reply_rotation_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-callback-numbers",
            post(crate::maintenance::backfill_callback_numbers_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-payment-accounts",
            post(crate::maintenance::backfill_payment_accounts_route),
        )
        // Queues stored mail carrying a PDF for the text worker.
        .route(
            "/v1/admin/maintenance:backfill-attachment-text",
            post(crate::maintenance::backfill_attachment_text_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-sections",
            post(crate::maintenance::backfill_sections_route),
        )
        // Counts the attachments of mail stored before they were counted.
        .route(
            "/v1/admin/maintenance:backfill-attachment-counts",
            post(crate::maintenance::backfill_attachment_counts_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-ledger",
            post(crate::maintenance::backfill_ledger_route),
        )
        .route(
            "/v1/admin/maintenance:backfill-thread-user",
            post(backfill_thread_user_route),
        )
        // The flag indexes key on markers present only while a column is
        // non-zero; these fill them in on older rows and check they agree.
        .route(
            "/v1/admin/maintenance:rewrite-threaduser-markers",
            post(crate::maintenance::rewrite_threaduser_markers_route),
        )
        .route(
            "/v1/admin/maintenance:threaduser-marker-check",
            post(crate::maintenance::threaduser_marker_check_route),
        )
        // Writes the group column onto rows that predate it, so the shadow
        // above can start reporting a defect rather than a debt.
        .route(
            "/v1/admin/maintenance:group-backfill",
            post(group_backfill_route),
        )
        // Own mail stored without `\Seen`, set seen once so that counting
        // own unseen mail as unread turns nothing unread on its own.
        .route(
            "/v1/admin/maintenance:settle-own-unseen",
            post(crate::maintenance::settle_own_unseen_route),
        )
        // The two declared columns that decide which list a thread lands
        // in, against the engine's per-user counts. Read-only.
        .route("/v1/admin/maintenance:axis-shadow", post(axis_shadow_route))
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
        // The list of addresses each user sent mail to, from the
        // relationship counters, for the fraud rules (dry by default).
        .route(adm::PATH_BACKFILL_SENT_TO, post(backfill_sent_to_route))
        // Importance verdicts for threads that predate the feature —
        // scoring only runs at ingest, so without this every existing
        // thread would stay blank forever.
        .route(
            "/v1/admin/maintenance:backfill-thread-importance",
            post(backfill_thread_importance_route),
        )
        // Each reader's own verdict back on their row, from their decision
        // log or a rescore of their own mail. `dry_run` unless
        // `dry_run=false`; `skip` / `limit` batch it.
        .route(
            "/v1/admin/maintenance:recompute-thread-importance",
            post(crate::maintenance::recompute_thread_importance_route),
        )
        // Each reader's participant list back to the senders of their own
        // messages. `dry_run` unless `dry_run=false`; `skip` / `limit`
        // batch it; multi-reader conversations unless `all=true`.
        .route(
            "/v1/admin/maintenance:recompute-thread-participants",
            post(crate::maintenance::recompute_thread_participants_route),
        )
        // Each reader's `unread` column back to what their own message rows
        // say. Same query shape as the participants recompute.
        .route(
            "/v1/admin/maintenance:recompute-unread-column",
            post(crate::maintenance::recompute_unread_column_route),
        )
        // Each reader's subject and preview back to their own messages,
        // with counts of what rows and the retired search index showed of
        // other people's mail. Same query shape as the two above.
        .route(
            "/v1/admin/maintenance:recompute-thread-display",
            post(crate::maintenance::recompute_thread_display_route),
        )
        // Each reader's tab and time back to their own messages, with
        // per-account counts of what would move tab or re-sort. Same query
        // shape as the three above.
        .route(
            "/v1/admin/maintenance:recompute-thread-standing",
            post(crate::maintenance::recompute_thread_standing_route),
        )
        // Stored message dates back to what ingest computes from the file,
        // with each reader's row time and the shared latest_date recounted.
        // Same batching as the four above.
        .route(
            "/v1/admin/maintenance:recompute-message-dates",
            post(crate::maintenance::recompute_message_dates_route),
        );
    #[cfg(feature = "measure-hooks")]
    let walks = walks.route(
        "/v1/admin/debug:run-periodic",
        post(crate::measure_hooks::run_periodic),
    );
    // one admission per router: each router counts only the requests it runs
    let admission = Arc::new(crate::maintenance::offload::Admission::default());
    r.merge(walks.route_layer(axum::middleware::from_fn_with_state(
        admission,
        crate::maintenance::offload::off_serving_runtime,
    )))
}
