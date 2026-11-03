//! The quota usage counter against what each account's maildir takes on
//! disk: `GET` reports the drift, `POST` with `dry_run=false` sets each
//! counter to the disk figure, only where nothing changed it meanwhile.

use serde::{Deserialize, Serialize};

/// `GET` reports; `POST ?dry_run=false` corrects. A `POST` without
/// `dry_run` only reports, as `quota-limits` does.
pub const PATH_USAGE_DRIFT: &str = "/v1/admin/maintenance:usage-drift";

/// What the pass did with one account's counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageDriftAction {
    /// The counter already says what the disk does; left alone.
    Equal,
    /// The counter differs; a dry run leaves it.
    WouldWrite,
    /// Set to the disk figure.
    Written,
    /// The counter moved while the maildir was being measured (mail
    /// arrived or was removed), so the figure may already be stale: left
    /// for the next pass.
    SkippedRaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageDriftRow {
    pub account: String,
    /// The counter before the pass; `None` when the account had none.
    pub counter: Option<i64>,
    /// Bytes its maildir takes; 0 for a counter without a maildir.
    pub actual: u64,
    /// `counter - actual`; `None` without a counter.
    pub diff: Option<i64>,
    pub action: UsageDriftAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageDriftResponse {
    pub dry_run: bool,
    /// Accounts looked at, with a maildir or a counter: what tells
    /// "nothing to do" from "read nothing".
    pub accounts: usize,
    /// Accounts whose counter is not the disk figure, missing ones included.
    pub differing: usize,
    /// Accounts a correction raises: counter below the disk figure, or
    /// missing while the maildir holds mail.
    pub counter_low: Vec<String>,
    /// Counters set to the disk figure by this pass.
    pub written: usize,
    /// Counters left because they moved during the measurement; run the
    /// pass again until this is 0.
    pub skipped_raced: usize,
    /// The accounts counted in `skipped_raced`, orphan counters included.
    pub raced: Vec<String>,
    pub rows: Vec<UsageDriftRow>,
    /// Counters whose address is no longer an account: nothing reads
    /// them, so the correction deletes them.
    pub orphan_counters: Vec<OrphanCounter>,
    /// Orphan counters deleted by this pass.
    pub orphans_deleted: usize,
}

/// A usage counter left behind for an address that is not an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanCounter {
    pub address: String,
    /// Its value when read; `None` when it was not a number.
    pub counter: Option<i64>,
    /// `WouldWrite` on a dry run, `Written` when deleted, `SkippedRaced`
    /// when it moved before the delete.
    pub action: UsageDriftAction,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_keep_their_wire_spelling() {
        let names: Vec<String> = [
            UsageDriftAction::Equal,
            UsageDriftAction::WouldWrite,
            UsageDriftAction::Written,
            UsageDriftAction::SkippedRaced,
        ]
        .iter()
        .map(|a| serde_json::to_string(a).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"equal\"",
                "\"would_write\"",
                "\"written\"",
                "\"skipped_raced\""
            ]
        );
    }
}
