//! `mailrs-mailbox-kevy` — kevy-backed mailbox store (experimental).
//!
//! Phase 7 of the 4-process split (checklist
//! `.claude/notes/arch-split-plus-fastcore-checklist-2026-06-30.md` §7).
//!
//! ## Design rationale
//!
//! The PG-backed cascade (`docs/CURRENT_STATE_FROZEN.md` Rock 1) lives in
//! `list_conversations` and the related thread-aggregate SQL. This crate
//! eliminates it structurally by:
//!
//! 1. Storing the **thread** as the source of truth (a hash per thread
//!    holding pre-aggregated counts, latest_date, senders csv, preview,
//!    category, importance — written once on each message arrival, never
//!    recomputed on read).
//! 2. Maintaining **secondary indexes as ZSETs** keyed by activity time,
//!    archive state, category, etc. List queries become
//!    `ZREVRANGE + N × HGETALL`, all O(log n).
//! 3. Full-text search via the kevy text index (Rocks 3 + 4
//!    from the feasibility note).
//!
//! ## KV layout
//!
//! ```text
//!   mailrs:thread:<tid>             hash  — aggregated thread state
//!     subject, senders_csv, count, unread_count, latest_date,
//!     latest_preview, category, importance_level, importance_score,
//!     requires_action, pinned, archived, has_action, sent_count
//!
//!   mailrs:user:<u>:threads:by_activity   zset (tid → max_date)
//!   mailrs:user:<u>:threads:pinned        zset
//!   mailrs:user:<u>:threads:archived      zset
//!   mailrs:user:<u>:threads:by_category:<cat>      zset
//!   mailrs:user:<u>:threads:has_unread:non_spam    zset
//!   mailrs:user:<u>:threads:has_action             zset
//!
//!   mailrs:mailbox:<id>             hash  — mailbox metadata
//!     name, user, uidvalidity, uidnext, highest_modseq
//!   mailrs:user:<u>:mailboxes       zset  — name → id
//!
//!   mailrs:message:<id>             hash  — full message row
//!   mailrs:mailbox:<id>:messages    zset  — uid → message_id
//!   mailrs:message:by-message-id:<u>:<mid>  string — message_id index
//!   mailrs:message:by-maildir:<u>:<mb>:<mid>  string — maildir index
//! ```
//!
//! ## Write-path fan-out
//!
//! Every message arrival (`insert_message` / `index_delivered`) triggers
//! a MULTI/EXEC block touching ~15 keys; the invariant checker in
//! `tests::invariants` validates thread state correctness after.
//!
//! ## Status
//!
//! **Scaffold** — only the bare `KevyMailboxStore` struct + a placeholder
//! `MailboxStore` trait impl wired. Per-method implementations land over
//! checklist 7.4–7.9 as separate commits.

#![allow(missing_docs)]
#![allow(dead_code)]

use std::io;
use std::sync::Arc;

use kevy_embedded::Store;

mod account;
mod alias;
mod deliver;
mod domain;
mod importance;
pub mod keys;
mod list_threads;
mod mark_seen;
pub mod mbid;
mod message_arrival;
mod messages;
mod move_category;
mod mutations;
mod rethread;
mod thread_row;
pub use list_threads::ListThreadsFilter;
pub use mailrs_mailbox::threading::normalize_subject;
pub use message_arrival::MessageArrival;
pub use thread_row::{ThreadRow, senders_csv_contains_user};

/// Experimental kevy-backed implementation of `MailboxStore`.
///
/// Construct with `KevyMailboxStore::new(store)` where `store` is the
/// shared `Arc<kevy_embedded::Store>` (in-process). Use under fastcore;
/// not currently swappable into the monolith core (Phase 8 fastcore
/// binary mounts this behind the same `mailrs-core-api` server surface).
#[derive(Clone)]
pub struct KevyMailboxStore {
    store: Arc<Store>,
}

impl KevyMailboxStore {
    pub fn new(store: Arc<Store>) -> Self {
        let s = Self { store };
        // v2.6.0 §P6: register the admin-CRUD range indexes idempotently
        // on construction. Callers no longer need to invoke
        // `ensure_admin_indexes()` explicitly. Duplicate declarations
        // (subsequent boots) are swallowed by `idx_create`'s own
        // idempotency contract.
        s.ensure_admin_indexes();
        s
    }

    /// Access to the inner kevy store — handed to MULTI/EXEC blocks in
    /// the per-method implementations.
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// Public reference to the inner store — for callers (like the
    /// fastcore binary) that need to run ad-hoc ZCARD / HGETALL outside
    /// the typed `mailbox-kevy` method surface. Stable: this returns
    /// the same store the typed methods use.
    pub fn store_ref(&self) -> &Store {
        &self.store
    }

    /// v2.6.0 §P6 dual-write: register the admin-CRUD range indexes
    /// idempotently at boot. Duplicate declarations return an
    /// InvalidInput error which we swallow — the catalog persists the
    /// spec on first call and refuses to re-declare on subsequent
    /// boots. Callers should invoke once during startup after the
    /// store handle is available.
    /// Fill in membership rows for threads that predate the table.
    ///
    /// Returns `(scanned, written)`. **Only writes where the row is
    /// absent or a field differs** — a second run over converged data
    /// writes nothing and reports `written == 0`, which is what makes
    /// it safe to call repeatedly and what
    /// `periodic-work-must-converge` asks for. An idempotent hset that
    /// rewrites identical bytes is idempotent but not convergent; the
    /// difference is the whole point of that rule.
    ///
    /// `offset` / `limit` page through one user's activity index so the
    /// caller can spread the work across ticks instead of holding the
    /// shard against live traffic for a single long sweep.
    pub fn backfill_thread_user(
        &self,
        user: &str,
        offset: i64,
        limit: i64,
    ) -> io::Result<(u64, u64)> {
        let activity = keys::user_threads_by_activity(user);
        let entries = self
            .store()
            .zrevrange(activity.as_bytes(), offset, offset + limit - 1)
            .map_err(std::io::Error::other)?;
        let mut scanned = 0u64;
        let mut written = 0u64;
        for (tid_bytes, _score) in entries {
            let Ok(tid) = String::from_utf8(tid_bytes) else {
                continue;
            };
            scanned += 1;
            let Some(row) = self.get_thread(&tid)? else {
                continue;
            };
            if self.write_thread_user_if_changed(user, &row)? {
                written += 1;
            }
        }
        Ok((scanned, written))
    }

    /// Declare the `threaduser` table — the access paths the engine
    /// maintains in place of twelve hand-written zsets.
    ///
    /// Idempotent: a second declaration of the same spec is refused by
    /// the catalog, which persisted it on the first call. Declaration
    /// is atomic — on any error nothing installs, so there is no
    /// half-declared table to clean up.
    ///
    /// **Nothing reads these paths yet.** The zsets remain
    /// authoritative through the backfill and the shadow-read window;
    /// this only starts the engine maintaining a parallel answer that
    /// `TABLE.VERIFY` can be checked against.
    ///
    /// Two ORDERPATHs cover the two orderings the UI actually asks for
    /// — the mutually-exclusive bucket (inbox / notifications /
    /// promotions / junk) and the finer category — both newest-first.
    /// The six orthogonal flags are stored as index VALUES instead of
    /// getting paths of their own, so "starred within inbox" is a
    /// FILTER over a path that already exists rather than a third
    /// composite.
    ///
    /// NOTE: `kevy-index` is imported directly because `kevy-embedded`
    /// 4.0.0 does not re-export `TableSpec` / `TableIndex` /
    /// `OrderPath`, which makes its own `Store::table_declare`
    /// uncallable through the facade. Drop this dependency once the
    /// re-export lands upstream.
    pub fn ensure_thread_table(&self) {
        use kevy_index::{IndexKind, OrderPath, TableIndex, TableSpec, ValType};

        fn col(name: &str, t: ValType) -> (Vec<u8>, ValType) {
            (name.as_bytes().to_vec(), t)
        }
        fn path(name: &str, on: &[(&str, bool)]) -> OrderPath {
            OrderPath {
                name: name.as_bytes().to_vec(),
                on: on
                    .iter()
                    .map(|(c, desc)| (c.as_bytes().to_vec(), *desc))
                    .collect(),
            }
        }
        let values: Vec<Vec<u8>> = ["user", "activity"]
            .iter()
            .map(|c| c.as_bytes().to_vec())
            .collect();

        let spec = TableSpec {
            name: b"threaduser".to_vec(),
            prefix: keys::THREAD_USER_PREFIX.to_vec(),
            pk: b"tid".to_vec(),
            columns: vec![
                col("user", ValType::Str),
                col("tid", ValType::Str),
                col("bucket", ValType::Str),
                col("category", ValType::Str),
                col("activity", ValType::I64),
                col("sent", ValType::I64),
                col("starred", ValType::I64),
                col("archived", ValType::I64),
                col("pinned", ValType::I64),
                col("unread", ValType::I64),
                col("has_action", ValType::I64),
            ],
            indexes: vec![
                TableIndex {
                    column: b"starred".to_vec(),
                    kind: IndexKind::Range,
                    values: values.clone(),
                },
                TableIndex {
                    column: b"archived".to_vec(),
                    kind: IndexKind::Range,
                    values,
                },
            ],
            // `ord` is the tie-breaker, not a queryable dimension.
            // `activity` is a whole-second timestamp, so threads that
            // arrive in the same second collide — 929 collisions over
            // 30k rows on prod. Without a total order the position of a
            // colliding row is undefined between calls, which is how a
            // paged reader silently skips or repeats one at a page
            // boundary.
            orderpaths: vec![
                path(
                    "by_user_bucket",
                    &[
                        ("user", false),
                        ("bucket", false),
                        ("activity", true),
                        ("ord", false),
                    ],
                ),
                path(
                    "by_user_category",
                    &[
                        ("user", false),
                        ("category", false),
                        ("activity", true),
                        ("ord", false),
                    ],
                ),
            ],
        };
        // The catalog persists across boots, so a redeclaration is
        // rejected rather than applied — which would silently pin the
        // table to whatever shape the first boot happened to declare.
        // Compare and rebuild only on a real change.
        let current = self
            .store
            .table_list()
            .into_iter()
            .find(|t| t.name == spec.name);
        match current {
            Some(ref existing) if *existing == spec => {
                tracing::debug!("threaduser table already matches");
                return;
            }
            Some(_) => {
                tracing::info!("threaduser table spec changed — rebuilding indexes");
                self.store.table_drop(&spec.name);
            }
            None => {}
        }
        match self.store.table_declare(spec) {
            Ok(()) => tracing::info!("threaduser table declared"),
            Err(e) => tracing::error!(error = %e, "threaduser table declaration failed"),
        }
    }

    pub fn ensure_admin_indexes(&self) {
        use kevy_embedded::{IndexKind, IndexValType};
        let s = &self.store;
        // Full-text over the synthesised `search_blob` field. kevy
        // maintains this from its commit hook, so it cannot drift from
        // the rows the way an external search service does — which is
        // exactly how conversation search ended up dead for weeks
        // (index name mismatch + dropped writes, 2026-07-19).
        // Dictionary-free CJK bigrams, so Japanese subjects and sender
        // names are searchable without an analyzer.
        let _ = s.idx_create(
            keys::IDX_THREAD_SEARCH,
            keys::THREAD_PREFIX,
            keys::THREAD_SEARCH_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        // Message bodies, indexed separately from the thread rows so a
        // long conversation doesn't rewrite one ever-growing value on
        // every arrival.
        let _ = s.idx_create(
            keys::IDX_MESSAGE_TEXT,
            keys::MSGTEXT_PREFIX,
            keys::MESSAGE_TEXT_FIELD,
            IndexValType::Str,
            IndexKind::Text,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_DOMAIN,
            keys::ALIAS_V2_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ALIASES_BY_TARGET,
            keys::ALIAS_V2_PREFIX,
            b"target",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_DOMAINS_BY_CREATED,
            keys::DOMAIN_V2_PREFIX,
            b"created_at",
            IndexValType::I64,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_DOMAIN,
            keys::ACCOUNT_PREFIX,
            b"domain",
            IndexValType::Str,
            IndexKind::Range,
        );
        let _ = s.idx_create(
            keys::IDX_ACCOUNTS_BY_ACTIVE,
            keys::ACCOUNT_PREFIX,
            b"active",
            IndexValType::Str,
            IndexKind::Range,
        );
    }
}

// MailboxStore trait impl + per-method bodies land in subsequent loops.
// For now we expose only the constructor so the fastcore binary can
// instantiate it; calling any method will panic until 7.5+ ships.
