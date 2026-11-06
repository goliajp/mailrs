//! `GET /v1/ops/status` — what the ops daemon reads from a core beyond its
//! health answer: how far the spool drain has got, and the envelope facts
//! of what was quarantined.
//!
//! It is not behind `MAILRS_CORE_API_SECRET`. The daemon holds its own
//! token, `MAILRS_OPS_TOKEN`, and a core is given only its sha256 as
//! `MAILRS_OPS_TOKEN_SHA256`, so the environment of a core does not hand
//! out the daemon's credential.

use serde::{Deserialize, Serialize};

pub const PATH_OPS_STATUS: &str = "/v1/ops/status";

/// Where the image fetcher's reports are handed to the core, which passes
/// them on to the daemon unchanged. fetch shares a network with the core
/// alone; the report stays signed with fetch's own key, so the core can
/// pass it on but not write one.
pub const PATH_OPS_RELAY: &str = "/v1/ops/reports:relay";

/// Sent by the daemon on its first status read after it starts; the core
/// records that read in the audit log, so the log shows the daemon came up.
pub const FIRST_READ_HEADER: &str = "x-mailrs-ops-first-read";

static FETCH_STARTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A fetch start report went through this core's relay.
pub fn witness_fetch_start() {
    FETCH_STARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// fetch start reports relayed since this core started. fetch keeps no
/// counter of its own (its only writable space is a tmpfs), so this is a
/// weaker reading: what the core saw pass through, unverified, not what
/// fetch counted.
pub fn fetch_starts_witnessed() -> u64 {
    FETCH_STARTS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Quarantine records listed at most; `quarantined_total` counts them all.
pub const MAX_QUARANTINE_RECORDS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpsStatusWire {
    /// The answering core's version, commit and start time. Served here,
    /// behind the daemon's token, rather than in the health answer.
    pub build: mailrs_ops_report::build::BuildInfo,
    /// fetch start reports relayed through this core since it started; see
    /// [`fetch_starts_witnessed`].
    pub fetch_starts_witnessed: u64,
    pub drain: DrainProgressWire,
    pub quarantined_total: u64,
    /// Newest first, at most [`MAX_QUARANTINE_RECORDS`].
    pub quarantined: Vec<QuarantinedWire>,
}

/// The spool drain's progress since this process started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainProgressWire {
    /// Drain passes completed.
    pub ticks: u64,
    /// Spool files delivered.
    pub delivered: u64,
    /// Unix seconds the last pass completed; `None` before the first.
    pub last_tick_at: Option<i64>,
}

/// One quarantined spool file, from the record beside it: the envelope and
/// the size, never the message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantinedWire {
    pub file: String,
    pub quarantined_at: i64,
    pub reverse_path: String,
    pub forward_paths: Vec<String>,
    pub size: u64,
}

/// Who may read the route, decided once when the router is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsAuth {
    /// A bearer token whose sha256 (lowercase hex) is this.
    TokenSha256(String),
    /// Nobody: no token hash is configured and the core RPC has a secret,
    /// so this is a real deployment that has not set the daemon up.
    Closed,
    /// Anybody: neither is configured (tests, local runs), matching the
    /// business routes, which are open without a secret.
    Open,
}

impl OpsAuth {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self::decide(
            var("MAILRS_OPS_TOKEN_SHA256").as_deref(),
            var(crate::AUTH_SECRET_ENV).is_some(),
        )
    }

    pub fn decide(token_sha256: Option<&str>, core_secret_set: bool) -> Self {
        match (token_sha256, core_secret_set) {
            (Some(h), _) => OpsAuth::TokenSha256(h.trim().to_ascii_lowercase()),
            (None, true) => OpsAuth::Closed,
            (None, false) => OpsAuth::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_hash_wins_and_a_bare_deployment_is_closed() {
        assert_eq!(
            OpsAuth::decide(Some(" ABC "), true),
            OpsAuth::TokenSha256("abc".into())
        );
        assert_eq!(OpsAuth::decide(None, true), OpsAuth::Closed);
        assert_eq!(OpsAuth::decide(None, false), OpsAuth::Open);
    }
}
