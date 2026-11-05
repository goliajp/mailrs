//! One cycle's readings. Every reading is a value or "could not read",
//! never a default standing in for one.

mod book;
mod collect;
mod pull;

#[cfg(test)]
pub mod fixture;

use std::collections::BTreeMap;

use mailrs_core_api::method::ops::OpsStatusWire;
use mailrs_core_api::types::HealthResponse;
use mailrs_ops_report::wire::Report;

use crate::store::Maintenance;

pub use book::{Rejection, ReportBook};
pub use collect::Collector;
pub use pull::Polls;

#[derive(Debug, Clone, PartialEq)]
pub enum Reading<T> {
    Got(T),
    Unreadable(String),
}

impl<T> Reading<T> {
    pub fn got(&self) -> Option<&T> {
        match self {
            Reading::Got(v) => Some(v),
            Reading::Unreadable(_) => None,
        }
    }

    pub fn why(&self) -> &str {
        match self {
            Reading::Got(_) => "",
            Reading::Unreadable(w) => w,
        }
    }
}

/// The latest word from one process.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessView {
    /// Whether its heartbeat is recent enough to read it by; always false
    /// for a process that does not push heartbeats.
    pub fresh: bool,
    pub age_secs: i64,
    pub latest: Report,
    pub instances: Vec<InstanceView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceView {
    pub instance: String,
    /// Starts in the last 15 minutes, reported or inferred.
    pub starts_15m: u64,
    /// Seconds since the latest start of the process's current instance,
    /// while no ready report has come for it.
    pub waiting_ready_secs: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelView {
    pub name: String,
    pub consecutive_failures: u32,
}

/// Thresholds the deployment sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub maintenance_alert_secs: i64,
    pub new_domains_per_day: u64,
    pub ready_secs: i64,
    pub fastcore_ready_secs: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub now: i64,
    pub fastcore: Reading<HealthResponse>,
    pub fastcore_ready: Reading<HealthResponse>,
    pub fastcore_ops: Reading<OpsStatusWire>,
    pub webapi: Reading<serde_json::Value>,
    pub webapi_ready: Reading<bool>,
    /// Seconds the drain's delivered count has stood still; `None` until
    /// two readings of it exist.
    pub drain_still_secs: Option<i64>,
    /// Growth of `failures_24h` over the last hour, per side.
    pub core_parse_increase: Option<u64>,
    pub receiver_parse_increase: Option<u64>,
    pub processes: BTreeMap<String, ProcessView>,
    pub push_sources: Vec<String>,
    /// `Got(true)` while kevy-server is reachable.
    pub kevy: Reading<bool>,
    pub channels: Vec<ChannelView>,
    pub heartbeat_configured: bool,
    pub maintenance: Option<Maintenance>,
    pub limits: Limits,
}

impl Snapshot {
    /// A snapshot with nothing read, for tests to fill in.
    pub fn empty(now: i64) -> Self {
        Snapshot {
            now,
            fastcore: Reading::Unreadable("not read".into()),
            fastcore_ready: Reading::Unreadable("not read".into()),
            fastcore_ops: Reading::Unreadable("not read".into()),
            webapi: Reading::Unreadable("not read".into()),
            webapi_ready: Reading::Unreadable("not read".into()),
            drain_still_secs: None,
            core_parse_increase: None,
            receiver_parse_increase: None,
            processes: BTreeMap::new(),
            push_sources: Vec::new(),
            kevy: Reading::Unreadable("not read".into()),
            channels: Vec::new(),
            heartbeat_configured: true,
            maintenance: None,
            limits: Limits {
                maintenance_alert_secs: 4 * 3600,
                new_domains_per_day: 5000,
                ready_secs: 600,
                fastcore_ready_secs: 1800,
            },
        }
    }

    /// A fresh process's latest report, by name.
    pub fn fresh(&self, process: &str) -> Option<&Report> {
        self.processes
            .get(process)
            .filter(|p| p.fresh)
            .map(|p| &p.latest)
    }

    /// The webapi health field at `path` (`a.b`), when webapi answered and
    /// the field is not null.
    pub fn webapi_field(&self, path: &str) -> Result<&serde_json::Value, crate::rules::Eval> {
        let Some(h) = self.webapi.got() else {
            return Err(crate::rules::Eval::down(format!(
                "webapi: {}",
                self.webapi.why()
            )));
        };
        let mut v = h;
        for part in path.split('.') {
            v = &v[part];
        }
        match v.is_null() {
            true => Err(crate::rules::Eval::field(format!("webapi {path} is null"))),
            false => Ok(v),
        }
    }
}
