//! RFC 7489 Appendix C data model for an inbound aggregate report.
//!
//! These types mirror the `<feedback>` schema receivers send to a `rua`
//! mailbox. They are deliberately permissive about optional elements:
//! the schema marks several fields optional, and real reporters differ
//! in which ones they emit. Anything the spec requires is a hard field;
//! everything else is an `Option` or defaults to empty.
//!
//! Numeric fields arrive as element text. `quick-xml`'s serde support
//! parses them directly, so a malformed number surfaces as a
//! deserialization error rather than a silently wrong value.

use serde::Deserialize;

/// A parsed DMARC aggregate report — one `<feedback>` document.
#[derive(Debug, Clone, Deserialize)]
pub struct AggregateReport {
    /// Who produced the report, and the window it covers.
    pub report_metadata: ReportMetadata,
    /// The DMARC policy the reporter observed for the domain.
    pub policy_published: PolicyPublished,
    /// Per-source rows. Absent when the reporter saw no mail at all,
    /// which is a legitimate (if unusual) report.
    #[serde(default, rename = "record")]
    pub records: Vec<ReportRecord>,
}

impl AggregateReport {
    /// Total messages covered by this report, summed across rows.
    pub fn total_count(&self) -> u64 {
        self.records.iter().map(|r| r.row.count).sum()
    }

    /// Messages that passed DMARC, summed across rows.
    pub fn passing_count(&self) -> u64 {
        self.records
            .iter()
            .filter(|r| r.passed())
            .map(|r| r.row.count)
            .sum()
    }
}

/// `<report_metadata>` — reporter identity and reporting window.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportMetadata {
    /// Reporting organization, e.g. `google.com`.
    pub org_name: String,
    /// Contact address for the reporting organization.
    pub email: String,
    /// Free-form extra contact info. Rarely populated.
    #[serde(default)]
    pub extra_contact_info: Option<String>,
    /// Reporter-assigned identifier, unique per (reporter, window).
    /// Used as the storage primary key so a re-sent report is a
    /// no-op rather than a double count.
    pub report_id: String,
    /// The window this report covers.
    pub date_range: DateRange,
}

/// `<date_range>` — unix-second bounds of the reporting window.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct DateRange {
    /// Window start, unix seconds.
    pub begin: i64,
    /// Window end, unix seconds.
    pub end: i64,
}

/// `<policy_published>` — what the reporter read out of the domain's
/// `_dmarc` TXT record at evaluation time.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyPublished {
    /// The domain the policy applies to.
    pub domain: String,
    /// DKIM alignment mode: `r` (relaxed) or `s` (strict).
    #[serde(default)]
    pub adkim: Option<String>,
    /// SPF alignment mode: `r` (relaxed) or `s` (strict).
    #[serde(default)]
    pub aspf: Option<String>,
    /// Requested policy: `none`, `quarantine`, or `reject`.
    pub p: String,
    /// Subdomain policy, when published separately from `p`.
    #[serde(default)]
    pub sp: Option<String>,
    /// Percentage of messages the policy was applied to.
    #[serde(default)]
    pub pct: Option<u8>,
}

/// One `<record>` — a group of messages sharing a source IP and verdict.
#[derive(Debug, Clone, Deserialize)]
pub struct ReportRecord {
    /// Source and evaluated policy for this group.
    pub row: Row,
    /// The domains this group claimed to be from.
    pub identifiers: Identifiers,
    /// Raw SPF / DKIM outcomes behind the evaluated verdict.
    #[serde(default)]
    pub auth_results: AuthResults,
}

impl ReportRecord {
    /// Whether this group passed DMARC.
    ///
    /// DMARC passes when *either* SPF or DKIM passes **and** aligns.
    /// `policy_evaluated` already encodes alignment, so reading it is
    /// correct — recomputing from `auth_results` would double-count
    /// unaligned passes.
    pub fn passed(&self) -> bool {
        self.row.policy_evaluated.dkim.eq_ignore_ascii_case("pass")
            || self.row.policy_evaluated.spf.eq_ignore_ascii_case("pass")
    }
}

/// `<row>` — source IP, message count, and the evaluated verdict.
#[derive(Debug, Clone, Deserialize)]
pub struct Row {
    /// Sending IP, v4 or v6, as the reporter saw it.
    pub source_ip: String,
    /// Number of messages in this group.
    pub count: u64,
    /// The verdict the reporter applied.
    pub policy_evaluated: PolicyEvaluated,
}

/// `<policy_evaluated>` — alignment-aware DMARC outcome plus the
/// disposition the reporter actually applied.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyEvaluated {
    /// What the reporter did: `none`, `quarantine`, or `reject`.
    pub disposition: String,
    /// Aligned DKIM outcome: `pass` or `fail`.
    pub dkim: String,
    /// Aligned SPF outcome: `pass` or `fail`.
    pub spf: String,
}

/// `<identifiers>` — the domains the message claimed.
#[derive(Debug, Clone, Deserialize)]
pub struct Identifiers {
    /// RFC 5321 MAIL FROM domain.
    #[serde(default)]
    pub envelope_from: Option<String>,
    /// RFC 5321 RCPT TO domain. Seldom emitted.
    #[serde(default)]
    pub envelope_to: Option<String>,
    /// RFC 5322 From header domain — the identity DMARC protects.
    pub header_from: String,
}

/// `<auth_results>` — the underlying SPF / DKIM checks. A record may
/// carry several of each (multiple signatures, or SPF checked against
/// both HELO and MAIL FROM).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuthResults {
    /// DKIM signature checks.
    #[serde(default)]
    pub dkim: Vec<DkimAuthResult>,
    /// SPF checks.
    #[serde(default)]
    pub spf: Vec<SpfAuthResult>,
}

/// One `<dkim>` entry inside `<auth_results>`.
#[derive(Debug, Clone, Deserialize)]
pub struct DkimAuthResult {
    /// The `d=` domain of the signature.
    pub domain: String,
    /// The `s=` selector, when reported.
    #[serde(default)]
    pub selector: Option<String>,
    /// Verification outcome: `pass`, `fail`, `neutral`, …
    pub result: String,
}

/// One `<spf>` entry inside `<auth_results>`.
#[derive(Debug, Clone, Deserialize)]
pub struct SpfAuthResult {
    /// The domain the SPF check ran against.
    pub domain: String,
    /// Which identity was checked: `helo` or `mfrom`.
    #[serde(default)]
    pub scope: Option<String>,
    /// Evaluation outcome: `pass`, `fail`, `softfail`, `none`, …
    pub result: String,
}
