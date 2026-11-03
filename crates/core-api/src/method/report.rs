//! Reporting a message as a scam: the reporter's copy is held at once, and
//! the other copies of the same template are taken back into Review when
//! the report has a ground, or wait for an administrator.

use serde::{Deserialize, Serialize};

/// `POST` — report one of the user's messages.
pub const PATH_REPORT_MESSAGE: &str = "/v1/users/{user}/messages/{message_id}/report";
/// `GET` — what became of one of the user's reports; 404 when it is not theirs.
pub const PATH_REPORT_OUTCOME: &str = "/v1/users/{user}/reports/{report_id}";
/// `GET` — reports whose other copies wait for an administrator.
pub const PATH_PENDING_REPORTS: &str = "/v1/admin/reports/pending";
/// `POST` — an administrator takes the other copies back.
pub const PATH_CONFIRM_REPORT: &str = "/v1/admin/reports/{report_id}/confirm";
/// `POST` — an administrator leaves the other copies where they are.
pub const PATH_DISMISS_REPORT: &str = "/v1/admin/reports/{report_id}/dismiss";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRequest {
    /// The reporter may judge mail for everyone; the caller decides.
    #[serde(default)]
    pub administrator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecideReportRequest {
    /// Who decided, for the audit.
    pub actor: String,
}

/// Where a report stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportState {
    /// The other copies were taken back into Review.
    TakenBack,
    /// No other inbox had a copy (or the body was too short to compare).
    NoneFound,
    /// The other copies wait for an administrator.
    Waiting,
    /// An administrator left the other copies where they were.
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportOutcomeWire {
    pub report_id: String,
    pub message_id: String,
    pub thread_id: String,
    pub state: ReportState,
    /// Conversations taken back from other inboxes.
    pub taken_back: u64,
    /// Other inboxes found holding a copy.
    pub inboxes: u64,
    /// `administrator`, `two_people`, `hold_rule`.
    pub grounds: Vec<String>,
    /// `no_ground`, `many_inboxes`, `long_history`, `allow_listed`.
    pub waits: Vec<String>,
    /// When it was reported, Unix seconds.
    pub at: i64,
    /// When the other copies were decided on; 0 while they wait.
    pub decided_at: i64,
}

/// One other copy, for an administrator to look at before confirming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSampleWire {
    pub user: String,
    pub subject: String,
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReportWire {
    pub outcome: ReportOutcomeWire,
    pub reporter: String,
    pub subject: String,
    pub from: String,
    pub sender_domain: String,
    /// Up to ten of the other copies.
    pub samples: Vec<ReportSampleWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingReportsResponse {
    pub items: Vec<PendingReportWire>,
}
