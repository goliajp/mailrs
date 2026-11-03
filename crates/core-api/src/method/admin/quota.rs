//! Per-account quota limits: the default every new account gets, and the
//! maintenance pass that gives it to accounts created without one.

use serde::{Deserialize, Serialize};

/// The limit a new account gets, in bytes (10 GiB). The one definition:
/// both cores write it when they create an account and when they fill in a
/// missing limit.
pub const DEFAULT_QUOTA_BYTES: i64 = 10 * 1024 * 1024 * 1024;

/// An account using more than this percentage of a limit is near full.
pub const NEAR_FULL_PERCENT: i64 = 80;

/// `POST` — list every account's usage against the default limit, and with
/// `?dry_run=false` write the default to accounts that have no limit.
/// Without the parameter nothing is written.
pub const PATH_QUOTA_LIMITS: &str = "/v1/admin/maintenance:quota-limits";

/// What the pass did, or would do, for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaLimitAction {
    /// A dry run: the default would be written.
    WouldWrite,
    /// The default was written.
    Written,
    /// The account already has a limit; left as it is.
    HasLimit,
    /// The account already uses more than [`NEAR_FULL_PERCENT`] of the
    /// default; left without a limit so the operator decides.
    #[serde(rename = "over_80")]
    Over80,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaLimitRow {
    pub address: String,
    /// The usage counter; 0 when there is none.
    pub used_bytes: i64,
    /// The limit before this pass; `None` when there was none.
    pub limit_bytes: Option<i64>,
    /// `used_bytes` / [`DEFAULT_QUOTA_BYTES`].
    pub used_ratio: f64,
    pub action: QuotaLimitAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaLimitsResponse {
    pub dry_run: bool,
    pub default_bytes: i64,
    /// Accounts looked at: what tells "nothing to do" from "read nothing".
    pub accounts: u64,
    /// Written, or with `dry_run` would be written.
    pub written: u64,
    pub skipped_has_limit: u64,
    pub skipped_over_80: u64,
    pub rows: Vec<QuotaLimitRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_ten_gibibytes() {
        assert_eq!(DEFAULT_QUOTA_BYTES, 10_737_418_240);
    }

    #[test]
    fn actions_keep_their_wire_spelling() {
        let names: Vec<String> = [
            QuotaLimitAction::WouldWrite,
            QuotaLimitAction::Written,
            QuotaLimitAction::HasLimit,
            QuotaLimitAction::Over80,
        ]
        .iter()
        .map(|a| serde_json::to_string(a).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"would_write\"",
                "\"written\"",
                "\"has_limit\"",
                "\"over_80\""
            ]
        );
    }
}
