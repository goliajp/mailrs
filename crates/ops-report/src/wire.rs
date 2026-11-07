//! The report a process pushes, and the fields each kind of process adds.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::build::BuildInfo;

/// The daemon's endpoint for reports.
pub const REPORTS_PATH: &str = "/v1/reports";
/// The header carrying `sha256=<hex>` of the request body.
pub const SIGNATURE_HEADER: &str = "x-mailrs-ops-signature";
/// The largest report body the daemon reads.
pub const MAX_REPORT_BYTES: usize = 64 * 1024;

/// Every process name a report may carry. `preflight` and `drill` are the
/// one-off commands of later phases; they have keys from the start so a
/// key handed out now does not change meaning later.
pub const PROCESSES: &[&str] = &[
    "fastcore",
    "webapi",
    "receiver",
    "sender",
    "fetch",
    "backup",
    // the daemon itself, for the alerts it hands the core
    "ops",
    "preflight",
    "drill",
];

/// One report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub process: String,
    /// Stable across restarts and container recreation: kept beside the
    /// start counter. A process without a state directory makes one up
    /// at start.
    pub instance: String,
    pub build: BuildInfo,
    /// How many times this instance has started, counting this start;
    /// `None` when the process has no state directory to count in.
    pub start_count: Option<u64>,
    /// Increases by one with every report of one start; the daemon drops a
    /// report whose number it has already seen.
    pub seq: u64,
    pub kind: ReportKind,
    /// Unix seconds this report was built.
    pub sent_at: i64,
    /// Unix seconds of this process's last completed kevy-server operation.
    pub kevy_contact_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Start,
    Ready,
    Heartbeat,
    Result,
}

impl ReportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ReportKind::Start => "start",
            ReportKind::Ready => "ready",
            ReportKind::Heartbeat => "heartbeat",
            ReportKind::Result => "result",
        }
    }
}

/// What a kind of process adds to its reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Detail {
    Receiver(ReceiverBeat),
    Sender(SenderBeat),
    Fetch(FetchBeat),
    Backup(BackupBeat),
    BackupResult(BackupResult),
    Webapi(WebapiBeat),
    /// fastcore's ready report: whether the boot replay came back intact.
    CoreReady {
        boot_intact: bool,
    },
}

/// The receiver's counters, as its stats frame carries them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverBeat {
    pub active_connections: u64,
    pub total_connections: u64,
    pub total_messages: u64,
    /// Its parse workers; `None` from a receiver that parses in process.
    pub parsing: Option<ParsingBeat>,
}

/// A process's parse workers, as its health probe reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsingBeat {
    pub failures_24h: u64,
    /// Unix seconds since which a worker has been held back.
    pub paused_since: Option<i64>,
}

/// The sender's progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderBeat {
    /// When the main loop last came back from waiting on the queue, with a
    /// job or without; `None` before the first time.
    pub last_pop_at: Option<i64>,
    /// When it last took a job.
    pub last_job_at: Option<i64>,
    /// Jobs waiting in the pending list; `None` when it could not be read.
    pub pending: Option<u64>,
    /// Age of the oldest waiting job; `None` when there is none or it could
    /// not be read.
    pub oldest_pending_age_secs: Option<u64>,
    /// Jobs finished since start, by how they ended.
    pub outcomes: BTreeMap<String, u64>,
    /// Ids of the ops daemon's alerts the receiving server accepted (250)
    /// from this sender, the latest last; the daemon counts an alert
    /// accepted only when it is here. Not a read receipt.
    #[serde(default)]
    pub ops_alerts_accepted: Vec<String>,
}

/// The image fetcher's counters since start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchBeat {
    pub fetched: u64,
    pub refused: u64,
    pub in_flight: u64,
}

/// The backup process between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupBeat {
    pub last_run_at: Option<i64>,
    pub last_ok: Option<bool>,
    pub last_success_at: Option<i64>,
}

/// One backup run, sent when it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupResult {
    pub ok: bool,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub reason: String,
}

/// The most distinct CSP reports one webapi start keeps.
pub const CSP_DISTINCT_MAX: usize = 300;

/// webapi's counters since its start: the content security policy reports
/// browsers sent, and the web pages it served, by client class.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebapiBeat {
    pub csp_reports: u64,
    pub csp_by_directive: BTreeMap<String, u64>,
    /// At most [`CSP_DISTINCT_MAX`] entries.
    pub csp_distinct: Vec<CspEntry>,
    /// Reports whose distinct key was new when the list was already full.
    pub csp_distinct_dropped: u64,
    pub pages_served: BTreeMap<String, u64>,
}

/// One distinct CSP report. Nothing here names a path past the first
/// segment or a query: mail image URLs carry ids and tokens, and app paths
/// carry thread ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CspEntry {
    /// The effective directive, or `other` for a name no policy has.
    pub directive: String,
    /// As the browser sent it, at most 200 characters.
    pub violated_directive: String,
    /// Scheme and host of the blocked URL, or the browser's keyword
    /// (`inline`, `eval`, `data`).
    pub blocked: String,
    /// The first path segment of the page, `""` for the root.
    pub page: String,
    /// `desktop`, `mobile`, `android-webview` or `unknown`.
    pub client: String,
    pub count: u64,
    /// Unix seconds.
    pub first_at: i64,
    pub last_at: i64,
}

impl CspEntry {
    /// What makes two reports the same entry.
    pub fn key(&self) -> (&str, &str, &str, &str, &str) {
        (
            &self.directive,
            &self.violated_directive,
            &self.blocked,
            &self.page,
            &self.client,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_round_trips_with_its_detail() {
        let r = Report {
            process: "sender".into(),
            instance: "abc".into(),
            build: crate::build::build(),
            start_count: Some(3),
            seq: 7,
            kind: ReportKind::Heartbeat,
            sent_at: 1_762_000_000,
            kevy_contact_at: None,
            detail: Some(Detail::Sender(SenderBeat {
                last_pop_at: Some(1),
                last_job_at: None,
                pending: Some(2),
                oldest_pending_age_secs: Some(30),
                outcomes: BTreeMap::from([("delivered".into(), 4)]),
                ops_alerts_accepted: vec!["1-1".into()],
            })),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"type\":\"sender\""), "{json}");
        assert_eq!(serde_json::from_str::<Report>(&json).unwrap(), r);
    }

    #[test]
    fn a_webapi_beat_round_trips() {
        let beat = WebapiBeat {
            csp_reports: 3,
            csp_by_directive: BTreeMap::from([("img-src".into(), 3)]),
            csp_distinct: vec![CspEntry {
                directive: "img-src".into(),
                violated_directive: "img-src".into(),
                blocked: "https://cdn.example.com".into(),
                page: "mail".into(),
                client: "desktop".into(),
                count: 3,
                first_at: 10,
                last_at: 20,
            }],
            csp_distinct_dropped: 1,
            pages_served: BTreeMap::from([("mobile".into(), 9)]),
        };
        let detail = Detail::Webapi(beat);
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains("\"type\":\"webapi\""), "{json}");
        assert_eq!(serde_json::from_str::<Detail>(&json).unwrap(), detail);
    }
}
