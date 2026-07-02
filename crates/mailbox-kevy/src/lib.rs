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
//! 3. Falling back to **meili** for semantic_search and FTS (Rocks 3 + 4
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

use std::sync::Arc;

use kevy_embedded::Store;

mod account;
mod deliver;
pub mod keys;
mod list_threads;
mod mark_seen;
mod message_arrival;
mod messages;
mod move_category;
mod mutations;
mod thread_row;
pub use list_threads::ListThreadsFilter;
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
        Self { store }
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
}

// MailboxStore trait impl + per-method bodies land in subsequent loops.
// For now we expose only the constructor so the fastcore binary can
// instantiate it; calling any method will panic until 7.5+ ships.
