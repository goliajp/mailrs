//! Admin maintenance routes — `/v1/admin/maintenance:*`.
//!
//! Split out of `lib.rs` on 2026-08-02, where they were 2,645 of its 6,371
//! lines. They belong together and away from the serving path: none of
//! them is on a request hot path, all of them are run by hand against
//! production, and reading one meant scrolling past the spool drain and
//! the IMAP wiring to reach it.
//!
//! Re-exported flat, so `lib.rs`'s route table still names the handlers
//! directly and the split is invisible at the registration site.

mod backfill_admin;
mod backfill_messages;
mod backfill_threading;
mod census;
mod cleanup;
mod idx_advice;
pub(crate) use idx_advice::idx_advice_route;
mod backfill_read_state;
mod reindex;
mod repair_blob_refs;
mod shadow_legacy;
mod shadow_messages;
mod shadow_projection;
mod shadow_read_state;
mod uidlist_ops;

pub(crate) use backfill_admin::*;
pub(crate) use backfill_messages::*;
pub(crate) use backfill_read_state::*;
pub(crate) use backfill_threading::*;
pub(crate) use census::*;
pub(crate) use cleanup::*;
pub(crate) use reindex::*;
pub(crate) use repair_blob_refs::*;
pub(crate) use shadow_legacy::*;
pub(crate) use shadow_messages::*;
pub(crate) use shadow_projection::*;
pub(crate) use shadow_read_state::*;
pub(crate) use uidlist_ops::*;

/// What every handler in here needs, in one place.
///
/// They are all the same shape — take the state, walk a population, answer
/// JSON — so the import list is the same too, and repeating it in three
/// files is three places to update.
mod prelude {
    pub(super) use std::sync::Arc;

    pub(super) use axum::Json;
    pub(super) use axum::extract::{Query, State};
    pub(super) use axum::response::IntoResponse;

    pub(super) use crate::FastcoreState;
    // Helpers that live beside the serving code in `lib.rs` and are read
    // from here. Private there, visible to a descendant module — the
    // split does not need to widen them.
    pub(super) use crate::maildir_scan::{
        UnionFind, from_header_domains, maildir_references, read_maildir_file,
        user_files_by_message_id,
    };
}
pub(crate) mod thread_date_audit;
