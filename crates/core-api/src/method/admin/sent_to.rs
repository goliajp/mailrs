//! The list of addresses each user has sent mail to, derived from the
//! relationship counters, which the fraud rules read as "someone the user
//! wrote to".

use serde::{Deserialize, Serialize};

/// `POST` — add every address whose `sent_count` is above zero to its
/// user's list, with `?dry_run=false`. Without the parameter nothing is
/// written. Adds only, so a second run finds nothing missing.
pub const PATH_BACKFILL_SENT_TO: &str = "/v1/admin/maintenance:backfill-sent-to";

/// What the pass found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentToBackfillResponse {
    pub dry_run: bool,
    /// Relationship records read.
    pub records: u64,
    /// Of those, the ones with `sent_count` above zero.
    pub sent_to: u64,
    /// Of those, the ones not yet in their user's list (added, unless dry).
    pub missing: u64,
}
