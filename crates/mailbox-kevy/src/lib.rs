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
        let spec = thread_user_spec();

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

/// The `threaduser` table declaration.
///
/// Split out of `ensure_thread_table` so the column/orderpath
/// agreement can be asserted in a test: kevy panics rather than
/// returning an error when an ORDERPATH names a column the table
/// never declared, and that panic happens at boot.
fn thread_user_spec() -> kevy_index::TableSpec {
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

    TableSpec {
        name: b"threaduser".to_vec(),
        prefix: keys::THREAD_USER_PREFIX.to_vec(),
        pk: b"tid".to_vec(),
        columns: vec![
            col("user", ValType::Str),
            col("tid", ValType::Str),
            col("ord", ValType::I64),
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
    }
}

#[cfg(test)]
mod table_spec_tests {
    use super::*;

    /// Every column an ORDERPATH or index sorts on must be declared.
    ///
    /// kevy resolves these with `column_type(col).expect("validated")`
    /// while compiling the table, so an undeclared column is a panic on
    /// the boot path — the process restart-loops rather than reporting
    /// a bad declaration. This landed on prod once (v2.12.5, rolled
    /// back in minutes) when a column was added to an orderpath and not
    /// to `columns`.
    #[test]
    fn orderpath_columns_are_declared() {
        let spec = thread_user_spec();
        let declared: std::collections::BTreeSet<Vec<u8>> =
            spec.columns.iter().map(|(n, _)| n.clone()).collect();

        for op in &spec.orderpaths {
            for (col, _) in &op.on {
                assert!(
                    declared.contains(col),
                    "orderpath {} sorts on undeclared column {}",
                    String::from_utf8_lossy(&op.name),
                    String::from_utf8_lossy(col)
                );
            }
        }
        for ix in &spec.indexes {
            assert!(
                declared.contains(&ix.column),
                "index keys on undeclared column {}",
                String::from_utf8_lossy(&ix.column)
            );
            for v in &ix.values {
                assert!(
                    declared.contains(v),
                    "index on {} stores undeclared column {}",
                    String::from_utf8_lossy(&ix.column),
                    String::from_utf8_lossy(v)
                );
            }
        }
    }

    /// The membership rows the write path produces must carry every
    /// declared column — a column present in the spec and absent from
    /// the row is a row the composite indexes silently exclude.
    #[test]
    fn written_fields_cover_declared_columns() {
        let row = thread_row::ThreadRow {
            thread_id: "t1".into(),
            subject: String::new(),
            senders_csv: String::new(),
            count: 1,
            unread_count: 0,
            latest_date: 1,
            latest_preview: String::new(),
            category: "inbox".into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
        };
        let written: std::collections::BTreeSet<Vec<u8>> =
            thread_row::thread_user_pairs("u@x.com", &row)
                .into_iter()
                .map(|(k, _)| k)
                .collect();
        for (col, _) in &thread_user_spec().columns {
            assert!(
                written.contains(col),
                "declared column {} is never written",
                String::from_utf8_lossy(col)
            );
        }
    }
}

impl KevyMailboxStore {
    /// Thread ids for one (user, bucket) pair, newest first, read from
    /// the declared ORDERPATH instead of a hand-maintained zset.
    ///
    /// `bucket` is the stored folder name (`inbox` / `notifications` /
    /// `promotions` / `junk`). Ordering comes from the composite
    /// encoding — `activity` is declared DESC, so the byte order the
    /// segment scans in is already the order the UI wants, and `ord`
    /// makes it total.
    pub fn list_thread_ids_by_bucket_via_table(
        &self,
        user: &str,
        bucket: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(b"threaduser.by_user_bucket", user, b"bucket", bucket, limit)
    }

    /// Same, keyed on the message category rather than the folder.
    pub fn list_thread_ids_by_category_via_table(
        &self,
        user: &str,
        category: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        self.query_orderpath(
            b"threaduser.by_user_category",
            user,
            b"category",
            category,
            limit,
        )
    }

    /// How many threads sit on one (user, bucket) axis.
    ///
    /// Counts in the index rather than materialising keys — the page
    /// total on a 1400-row axis should not cost 1400 key copies.
    pub fn count_thread_ids_by_bucket_via_table(
        &self,
        user: &str,
        bucket: &str,
    ) -> io::Result<usize> {
        let (lo, hi) = self.bucket_bounds(b"threaduser.by_user_bucket", user, b"bucket", bucket)?;
        self.store
            .idx_count(
                b"threaduser.by_user_bucket",
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
            )
            .map(|n| n as usize)
            .map_err(io::Error::other)
    }

    /// The cursor page: threads on this axis older than `max_activity`.
    ///
    /// `activity` is the component right after the two equality columns
    /// in the composite, which is the only position a range may
    /// constrain — so this is the shape the declared ORDERPATH was
    /// designed to answer, not a scan with a filter on top.
    pub fn list_thread_ids_by_bucket_before_via_table(
        &self,
        user: &str,
        bucket: &str,
        max_activity: i64,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (b"bucket".to_vec(), bucket.as_bytes().to_vec()),
            ],
            range: Some((
                b"activity".to_vec(),
                i64::MIN.to_string().into_bytes(),
                max_activity.to_string().into_bytes(),
            )),
        };
        self.run_orderpath(b"threaduser.by_user_bucket", user, &clause, limit)
    }

    fn bucket_bounds(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (second_col.to_vec(), second_val.as_bytes().to_vec()),
            ],
            range: None,
        };
        self.composite_bounds_for(index, &clause)
    }

    fn query_orderpath(
        &self,
        index: &[u8],
        user: &str,
        second_col: &[u8],
        second_val: &str,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        use kevy_index::WhereClause;
        let clause = WhereClause {
            eqs: vec![
                (b"user".to_vec(), user.as_bytes().to_vec()),
                (second_col.to_vec(), second_val.as_bytes().to_vec()),
            ],
            range: None,
        };
        self.run_orderpath(index, user, &clause, limit)
    }

    /// Resolve the composite columns for `index` from the declared
    /// spec, so the bounds are always encoded the way the segment was.
    fn composite_bounds_for(
        &self,
        index: &[u8],
        clause: &kevy_index::WhereClause,
    ) -> io::Result<(Vec<u8>, Vec<u8>)> {
        use kevy_index::{CompositeCol, composite_bounds};
        let spec = thread_user_spec();
        let path = spec
            .orderpaths
            .iter()
            .find(|p| index.ends_with(&p.name))
            .ok_or_else(|| io::Error::other("no such orderpath in the spec"))?;
        let cols: Vec<CompositeCol> = path
            .on
            .iter()
            .map(|(col, desc)| {
                let ty = spec
                    .column_type(col)
                    .ok_or_else(|| io::Error::other("orderpath names an undeclared column"))?;
                Ok(CompositeCol {
                    name: col.clone(),
                    ty,
                    desc: *desc,
                })
            })
            .collect::<io::Result<_>>()?;
        composite_bounds(&cols, clause).map_err(io::Error::other)
    }

    fn run_orderpath(
        &self,
        index: &[u8],
        user: &str,
        clause: &kevy_index::WhereClause,
        limit: usize,
    ) -> io::Result<Vec<String>> {
        let (lo, hi) = self.composite_bounds_for(index, clause)?;
        let (rows, _cursor) = self
            .store
            .idx_query(
                index,
                &kevy_embedded::IndexValue::Str(lo),
                &kevy_embedded::IndexValue::Str(hi),
                None,
                limit,
            )
            .map_err(io::Error::other)?;

        // The row key is `mailrs:threaduser:{user}:{tid}`; the tid can
        // itself contain colons (it is a Message-ID), so split off the
        // known prefix rather than splitting on the separator.
        let prefix_len = keys::thread_user(user, "").len();
        Ok(rows
            .into_iter()
            .filter_map(|(key, _)| {
                let k = String::from_utf8(key).ok()?;
                k.get(prefix_len..).map(str::to_string)
            })
            .collect())
    }
}

#[cfg(test)]
mod orderpath_read_tests {
    use super::*;
    use kevy_embedded::{Config, Store};

    fn row(tid: &str, activity: i64, category: &str) -> thread_row::ThreadRow {
        thread_row::ThreadRow {
            thread_id: tid.into(),
            subject: String::new(),
            senders_csv: String::new(),
            count: 1,
            unread_count: 0,
            latest_date: activity,
            latest_preview: String::new(),
            category: category.into(),
            importance_level: "normal".into(),
            importance_score: 0.0,
            requires_action: false,
            pinned: false,
            archived: false,
            has_action: false,
            sent_count: 0,
            starred: false,
        }
    }

    /// The engine's answer must be the order the UI asks for: newest
    /// first, scoped to one user and one bucket, with rows belonging to
    /// other users or other buckets absent.
    #[test]
    fn orderpath_returns_newest_first_scoped_to_the_user() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        for (tid, when) in [("old", 100), ("newest", 300), ("middle", 200)] {
            st.write_thread_user_if_changed("alice@x.com", &row(tid, when, "inbox"))
                .unwrap();
        }
        // A different user's copy of a thread, and a junk thread — both
        // must stay out of alice's inbox answer.
        st.write_thread_user_if_changed("bob@x.com", &row("bobs", 999, "inbox"))
            .unwrap();
        st.write_thread_user_if_changed("alice@x.com", &row("spammy", 999, "spam"))
            .unwrap();

        let got = st
            .list_thread_ids_by_bucket_via_table("alice@x.com", "inbox", 50)
            .unwrap();
        assert_eq!(got, vec!["newest", "middle", "old"]);
    }

    /// Threads whose ids exceed kevy's 255-byte string component cap
    /// must still be indexed — that is the whole reason the sort ends
    /// on a folded hash rather than on the id itself.
    #[test]
    fn an_overlong_thread_id_is_still_indexed() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();

        let long_tid = format!("<{}@example.com>", "x".repeat(300));
        st.write_thread_user_if_changed("alice@x.com", &row(&long_tid, 500, "inbox"))
            .unwrap();
        st.write_thread_user_if_changed("alice@x.com", &row("short", 100, "inbox"))
            .unwrap();

        let got = st
            .list_thread_ids_by_bucket_via_table("alice@x.com", "inbox", 50)
            .unwrap();
        assert_eq!(got, vec![long_tid, "short".to_string()]);
    }
}
