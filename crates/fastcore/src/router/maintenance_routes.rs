//! The maintenance sweeps, kept out of the main route table.
//!
//! Twenty-six routes that rebuild, audit or reclaim derived state.
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
    r.route("/v1/admin/maintenance:rewrite-aof", post(rewrite_aof_route))
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
        // them. Step 1 of `20260814-the-maildir-is-the-store.md`, run before
        // anything writes either side.
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
            "/v1/admin/maintenance:repair-blob-refs",
            post(repair_blob_refs_route),
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
        // Segmented promotion of existing threads into the
        // `threaduser` table's membership rows (v4 TABLE migration).
        // Paged on purpose: a full scan competes with live traffic
        // for the same store, so the caller drives it in batches.
        .route(
            "/v1/admin/maintenance:backfill-thread-user",
            post(backfill_thread_user_route),
        )
        // What the declared aggregate index counts, against what the rows
        // say. Read-only; the gate for moving the read onto the engine.
        .route(
            "/v1/admin/maintenance:count-shadow",
            post(count_shadow_route),
        )
        // Writes the group column onto rows that predate it, so the shadow
        // above can start reporting a defect rather than a debt.
        .route(
            "/v1/admin/maintenance:group-backfill",
            post(group_backfill_route),
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
        // Importance verdicts for threads that predate the feature —
        // scoring only runs at ingest, so without this every existing
        // thread would stay blank forever.
        .route(
            "/v1/admin/maintenance:backfill-thread-importance",
            post(backfill_thread_importance_route),
        )
}
