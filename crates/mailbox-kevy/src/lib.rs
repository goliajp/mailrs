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
mod recount;
mod rethread;
mod shadow_counts;
mod table_query;
mod table_query_tests;
mod table_spec;
mod thread_row;
pub use list_threads::ListThreadsFilter;
pub use mailrs_mailbox::threading::normalize_subject;
pub use message_arrival::MessageArrival;
pub use messages::{OwnedUserMessageFacts, UserMessageFacts};
pub use table_query::ArchiveScope;
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
        // The thread axes are READ from the declared table, so a store
        // that never declared it answers every thread query with
        // nothing — no error, just empty. That was left to the caller
        // and 34 call sites paired it with `new()` by hand; the ones
        // that forgot (fastcore's own test helper, the core-sync
        // round-trip, and all three `bin/` backfill tools) read empty
        // and reported success. Declaring here is idempotent — the
        // method early-returns when the live spec already matches — so
        // the pairing cannot come apart.
        s.ensure_thread_table();
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
        // The membership rows themselves. Every write path maintains
        // them now, so this no longer discovers threads — it refreshes
        // rows against the current schema, which is what a column
        // addition needs and nothing else does.
        //
        // (It read the union of the legacy zsets while those were
        // still written. They are not, so reading them would now
        // return a shrinking, stale set.)
        let prefix = keys::thread_user(user, "");
        let mut ids: Vec<String> = self
            .store()
            .keys(Some(format!("{prefix}*").as_bytes()), None)
            .into_iter()
            .filter_map(|k| {
                let k = String::from_utf8(k).ok()?;
                k.get(prefix.len()..).map(str::to_string)
            })
            .collect();
        ids.sort();
        let entries: Vec<String> = ids
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect();
        let mut scanned = 0u64;
        let mut written = 0u64;
        for tid in entries {
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
}

// MailboxStore trait impl + per-method bodies land in subsequent loops.
// For now we expose only the constructor so the fastcore binary can
// instantiate it; calling any method will panic until 7.5+ ships.

#[cfg(test)]
mod backfill_source_tests {
    use super::*;
    use kevy_embedded::{Config, Store};

    /// The backfill refreshes existing rows against the current
    /// schema — that is its whole job now.
    ///
    /// It used to enumerate the legacy zsets to *discover* rows the
    /// write paths had missed, and while those zsets were maintained
    /// that mattered: one prod account's `sent` held 58 threads where
    /// `by_activity` held 9, and reading either alone left the other's
    /// behind. Nothing writes them any more, so reading them would
    /// return a stale and shrinking set; the rows are the truth.
    #[test]
    fn backfill_refreshes_existing_rows_and_then_converges() {
        let st = KevyMailboxStore::new(Arc::new(
            Store::open(Config::default()).expect("open in-memory kevy"),
        ));
        st.ensure_thread_table();
        let u = "alice@x.com";

        let mut row = thread_row::ThreadRow {
            thread_id: "t1".into(),
            subject: "s".into(),
            senders_csv: "bob@y.com".into(),
            count: 1,
            unread_count: 0,
            latest_date: 100,
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
        st.upsert_thread(u, &row).unwrap();

        // Converged: a pass over correct rows writes nothing.
        let (scanned, written) = st.backfill_thread_user(u, 0, 500).unwrap();
        assert_eq!(scanned, 1);
        assert_eq!(written, 0, "a converged pass must not write");

        // Simulate a row left behind by an older schema: clear a
        // column the current builder produces.
        st.store()
            .hdel(keys::thread_user(u, "t1").as_bytes(), &[b"ord".as_slice()])
            .unwrap();
        let (scanned, written) = st.backfill_thread_user(u, 0, 500).unwrap();
        assert_eq!(scanned, 1);
        assert_eq!(written, 1, "a stale row must be rewritten");

        // And it stays converged afterwards.
        row.latest_date = 100;
        let (_, written) = st.backfill_thread_user(u, 0, 500).unwrap();
        assert_eq!(written, 0);
    }
}
