//! `POST /v1/ops/alerts` — the ops daemon's own path for alerts: the core
//! queues the alert as mail from the ops sending identity to the operator
//! addresses the core is configured with, and mailrs's own sender delivers
//! it (and the operator's apps hear of it as of any new mail).
//!
//! The daemon cannot name recipients: the request carries an id, a subject
//! and a text, nothing else, and the route does nothing but queue that to
//! `MAILRS_OPS_OPERATOR_ADDRESSES`. Its credential is the daemon's derived
//! key (`mailrs-ops report-key ops`), of which the core holds only the
//! sha256 (`MAILRS_OPS_ALERT_KEY_SHA256`). That key opens nothing else: it
//! reads no mailbox and changes no setting.
//!
//! A 200 here says the alert was queued, not that it arrived. The daemon
//! counts it delivered only when the sender's heartbeat lists its id.

use serde::{Deserialize, Serialize};

use super::ops::OpsAuth;

pub const PATH_OPS_ALERTS: &str = "/v1/ops/alerts";

/// The derived key's process name.
pub const OPS_PROCESS: &str = "ops";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsAlertWire {
    /// The daemon's message id; the sender reports it back once the
    /// receiving server accepted the mail (250).
    pub id: String,
    pub subject: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpsAlertQueued {
    /// One job per operator address.
    pub queued: usize,
}

/// Who the alerts go to and come from, as the core is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsOperators {
    pub from: String,
    pub to: Vec<String>,
}

impl OpsOperators {
    /// `MAILRS_OPS_ALERT_FROM` and `MAILRS_OPS_OPERATOR_ADDRESSES`; `None`
    /// unless both are set.
    pub fn from_env() -> Option<Self> {
        let from = std::env::var("MAILRS_OPS_ALERT_FROM")
            .ok()?
            .trim()
            .to_string();
        let to: Vec<String> = std::env::var("MAILRS_OPS_OPERATOR_ADDRESSES")
            .ok()?
            .split(',')
            .map(|a| a.trim().to_string())
            .filter(|a| a.contains('@'))
            .collect();
        (from.contains('@') && !to.is_empty()).then_some(OpsOperators { from, to })
    }
}

/// Everything the ops routes of a core are configured with.
#[derive(Debug, Clone)]
pub struct OpsConfig {
    /// Who may read `/v1/ops/status`.
    pub status: OpsAuth,
    /// Who may queue on `/v1/ops/alerts`.
    pub alerts: OpsAuth,
    pub operators: Option<OpsOperators>,
    /// The daemon's report endpoint fetch's reports are relayed to.
    pub relay_to: Option<String>,
}

impl OpsConfig {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let core_secret = var(crate::AUTH_SECRET_ENV).is_some();
        OpsConfig {
            status: OpsAuth::decide(var("MAILRS_OPS_TOKEN_SHA256").as_deref(), core_secret),
            alerts: OpsAuth::decide(var("MAILRS_OPS_ALERT_KEY_SHA256").as_deref(), core_secret),
            operators: OpsOperators::from_env(),
            relay_to: var("MAILRS_OPS_URL").map(|u| {
                format!(
                    "{}{}",
                    u.trim_end_matches('/'),
                    mailrs_ops_report::wire::REPORTS_PATH
                )
            }),
        }
    }

    /// Everything open, nothing configured: tests and local runs.
    pub fn open() -> Self {
        OpsConfig {
            status: OpsAuth::Open,
            alerts: OpsAuth::Open,
            operators: None,
            relay_to: None,
        }
    }
}
