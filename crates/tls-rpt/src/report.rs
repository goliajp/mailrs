//! RFC 8460 §4 JSON report data model + a builder that accumulates
//! per-connection event facts and emits an owned [`Report`] ready to
//! `serde_json::to_vec`.
//!
//! The builder is the "data architecture" entry point: each
//! observation (one TLS attempt to one MX) is recorded as an event
//! fact via [`ReportBuilder::record_success`] / [`record_failure`],
//! and [`ReportBuilder::build`] aggregates them per
//! (policy-domain, policy-type) bucket into the JSON shape
//! receivers expect.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

use crate::error::TlsRptError;
use crate::failure::FailureType;

/// Top-level TLSRPT report (one JSON document = one report).
///
/// Field naming matches RFC 8460 §4 exactly so `serde_json::to_string`
/// produces the wire format a TLSRPT receiver expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Free-text name of the reporting organization.
    #[serde(rename = "organization-name")]
    pub organization_name: String,
    /// The report's time window (inclusive start, exclusive end).
    #[serde(rename = "date-range")]
    pub date_range: DateRange,
    /// Free-text contact email / URL for follow-up questions.
    #[serde(rename = "contact-info")]
    pub contact_info: String,
    /// Unique identifier — receiver may use to deduplicate.
    #[serde(rename = "report-id")]
    pub report_id: String,
    /// One entry per (policy-domain, policy-type) bucket.
    pub policies: Vec<PolicyReport>,
}

/// Inclusive `start-datetime` / exclusive `end-datetime` window per
/// RFC 3339. Caller formats the strings (no chrono dependency).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    /// RFC 3339 timestamp marking the start of the reporting window.
    #[serde(rename = "start-datetime")]
    pub start_datetime: String,
    /// RFC 3339 timestamp marking the end of the reporting window.
    #[serde(rename = "end-datetime")]
    pub end_datetime: String,
}

/// One `policies[]` entry — counts + failure detail for a single
/// (receiving domain, applied policy) bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyReport {
    /// The policy that applied to these connections.
    pub policy: PolicyBlock,
    /// Aggregate success / failure session counts for the window.
    pub summary: SummaryBlock,
    /// Per-failure detail rows. Empty when `summary.total_failure_session_count == 0`.
    #[serde(
        rename = "failure-details",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub failure_details: Vec<FailureDetail>,
}

/// The `policy {}` sub-object inside a [`PolicyReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyBlock {
    /// `policy-type` — `sts` / `tlsa` / `no-policy-found`.
    #[serde(rename = "policy-type")]
    pub policy_type: PolicyType,
    /// Verbatim policy text (e.g. the STS policy body) when applicable.
    /// Per RFC 8460 §4.2 receivers send back the raw policy bytes so
    /// the owner can correlate against what they published.
    #[serde(
        rename = "policy-string",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub policy_string: Vec<String>,
    /// The receiving domain the policy applied to.
    #[serde(rename = "policy-domain")]
    pub policy_domain: String,
    /// MX hosts in scope for the policy (typically the receiver's
    /// resolved MX set, intersected with policy mx= patterns).
    #[serde(rename = "mx-host", default, skip_serializing_if = "Vec::is_empty")]
    pub mx_host: Vec<String>,
}

/// `policy-type` value. RFC 8460 §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyType {
    /// STS policy fetched and applied (RFC 8461).
    Sts,
    /// DANE TLSA policy applied (RFC 7672).
    Tlsa,
    /// No policy was applicable — used when the sender attempted
    /// opportunistic TLS without policy enforcement.
    NoPolicyFound,
}

/// Aggregate counts for one [`PolicyReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryBlock {
    /// Total TLS sessions in the window that succeeded against this policy.
    #[serde(rename = "total-successful-session-count")]
    pub total_successful_session_count: u64,
    /// Total sessions that failed.
    #[serde(rename = "total-failure-session-count")]
    pub total_failure_session_count: u64,
}

/// One `failure-details[]` row. RFC 8460 §4.3 — every reported
/// failure carries a result-type plus diagnostic context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureDetail {
    /// Why this group of sessions failed.
    #[serde(rename = "result-type")]
    pub result_type: FailureType,
    /// IP of the sending MTA (yours). Optional — set only when known.
    #[serde(
        rename = "sending-mta-ip",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sending_mta_ip: Option<IpAddr>,
    /// Hostname the receiving MX presented in DNS.
    #[serde(
        rename = "receiving-mx-hostname",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiving_mx_hostname: Option<String>,
    /// `EHLO/HELO` string the receiving MX advertised.
    #[serde(
        rename = "receiving-mx-helo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiving_mx_helo: Option<String>,
    /// IP we connected to.
    #[serde(
        rename = "receiving-ip",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub receiving_ip: Option<IpAddr>,
    /// How many sessions with these exact characteristics failed.
    #[serde(rename = "failed-session-count")]
    pub failed_session_count: u64,
    /// Free-text human-readable detail. Optional.
    #[serde(
        rename = "additional-information",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_information: Option<String>,
    /// Failure-reason-code: typically the underlying TLS alert code
    /// or STS fetch HTTP status.
    #[serde(
        rename = "failure-reason-code",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub failure_reason_code: Option<String>,
}

/// Per-connection event fact: success.
///
/// Captures everything needed to bucket the event by (policy-domain,
/// policy-type) and to emit MX-host counts. Designed to be cheap to
/// construct from a `TlsState`/`SmtpClient` connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccessEvent {
    /// The receiving domain this connection was for.
    pub policy_domain: String,
    /// Which kind of policy gated the connection.
    pub policy_type: PolicyType,
    /// MX hostname connected to.
    pub mx_host: String,
}

/// Per-connection event fact: failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureEvent {
    /// The receiving domain this connection was for.
    pub policy_domain: String,
    /// Which kind of policy gated the connection.
    pub policy_type: PolicyType,
    /// MX hostname connected to (when known).
    pub mx_host: Option<String>,
    /// Why it failed.
    pub result_type: FailureType,
    /// IP we initiated from.
    pub sending_mta_ip: Option<IpAddr>,
    /// IP we connected to.
    pub receiving_ip: Option<IpAddr>,
    /// EHLO/HELO from the receiver.
    pub receiving_mx_helo: Option<String>,
    /// Free-text reason / underlying error.
    pub additional_information: Option<String>,
    /// Underlying error code.
    pub failure_reason_code: Option<String>,
}

/// Builder that accumulates [`SuccessEvent`] / [`FailureEvent`] facts
/// and emits an owned [`Report`] on [`ReportBuilder::build`].
///
/// Bucketing rule (RFC 8460 §4.1):
/// - Successes group by `(policy_domain, policy_type)`.
/// - Failures group by `(policy_domain, policy_type, result_type,
///   sending_mta_ip, receiving_mx_hostname, receiving_ip,
///   receiving_mx_helo, additional_information,
///   failure_reason_code)` — every distinct context becomes its own
///   `failure-details[]` row with the count of matching events.
pub struct ReportBuilder {
    organization_name: Option<String>,
    contact_info: Option<String>,
    report_id: Option<String>,
    date_range: Option<DateRange>,
    // (policy_domain, policy_type) -> (success_count, mx_hosts)
    buckets: HashMap<(String, PolicyType), Bucket>,
    // Per-context failure counts, keyed inside each Bucket.
}

#[derive(Default)]
struct Bucket {
    success_count: u64,
    mx_hosts: Vec<String>,
    failures: HashMap<FailureKey, u64>,
    // Cached policy-string from the STS / TLSA fetch, when known.
    policy_string: Vec<String>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct FailureKey {
    result_type: FailureType,
    sending_mta_ip: Option<IpAddr>,
    receiving_mx_hostname: Option<String>,
    receiving_ip: Option<IpAddr>,
    receiving_mx_helo: Option<String>,
    additional_information: Option<String>,
    failure_reason_code: Option<String>,
}

impl ReportBuilder {
    /// Start a new builder. Required fields must be set before
    /// [`build`](Self::build).
    pub fn new() -> Self {
        Self {
            organization_name: None,
            contact_info: None,
            report_id: None,
            date_range: None,
            buckets: HashMap::new(),
        }
    }

    /// Set the sender's organization name (e.g. `"GOLIA K.K."`).
    pub fn organization_name(mut self, s: impl Into<String>) -> Self {
        self.organization_name = Some(s.into());
        self
    }

    /// Set the contact info field — usually a `mailto:` URL.
    pub fn contact_info(mut self, s: impl Into<String>) -> Self {
        self.contact_info = Some(s.into());
        self
    }

    /// Set the report-id. Must be unique to the sender so receivers
    /// can dedup retransmits.
    pub fn report_id(mut self, s: impl Into<String>) -> Self {
        self.report_id = Some(s.into());
        self
    }

    /// Set the reporting window in RFC 3339 timestamps.
    pub fn date_range(mut self, start: impl Into<String>, end: impl Into<String>) -> Self {
        self.date_range = Some(DateRange {
            start_datetime: start.into(),
            end_datetime: end.into(),
        });
        self
    }

    /// Stash the raw policy bytes (e.g. STS policy body) for the
    /// `(domain, type)` bucket. RFC 8460 §4.2 puts this in
    /// `policy-string` so receivers can correlate.
    pub fn policy_string(
        mut self,
        domain: impl Into<String>,
        ptype: PolicyType,
        lines: Vec<String>,
    ) -> Self {
        let key = (domain.into(), ptype);
        self.buckets.entry(key).or_default().policy_string = lines;
        self
    }

    /// Record one successful TLS session.
    pub fn record_success(&mut self, e: SuccessEvent) {
        let key = (e.policy_domain, e.policy_type);
        let bucket = self.buckets.entry(key).or_default();
        bucket.success_count += 1;
        if !bucket.mx_hosts.iter().any(|h| h == &e.mx_host) {
            bucket.mx_hosts.push(e.mx_host);
        }
    }

    /// Record one failed TLS session.
    pub fn record_failure(&mut self, e: FailureEvent) {
        let key = (e.policy_domain.clone(), e.policy_type);
        let bucket = self.buckets.entry(key).or_default();
        if let Some(h) = &e.mx_host
            && !bucket.mx_hosts.iter().any(|x| x == h)
        {
            bucket.mx_hosts.push(h.clone());
        }
        let fk = FailureKey {
            result_type: e.result_type,
            sending_mta_ip: e.sending_mta_ip,
            receiving_mx_hostname: e.mx_host,
            receiving_ip: e.receiving_ip,
            receiving_mx_helo: e.receiving_mx_helo,
            additional_information: e.additional_information,
            failure_reason_code: e.failure_reason_code,
        };
        *bucket.failures.entry(fk).or_insert(0) += 1;
    }

    /// Consume the builder and produce the [`Report`].
    pub fn build(self) -> Result<Report, TlsRptError> {
        let organization_name = self
            .organization_name
            .ok_or(TlsRptError::MissingField("organization_name"))?;
        let contact_info = self
            .contact_info
            .ok_or(TlsRptError::MissingField("contact_info"))?;
        let report_id = self
            .report_id
            .ok_or(TlsRptError::MissingField("report_id"))?;
        let date_range = self
            .date_range
            .ok_or(TlsRptError::MissingField("date_range"))?;

        // Stable bucket ordering for deterministic output.
        let mut keys: Vec<_> = self.buckets.keys().cloned().collect();
        keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| (a.1 as u8).cmp(&(b.1 as u8))));

        let mut policies = Vec::with_capacity(keys.len());
        let mut buckets = self.buckets;
        for (domain, ptype) in keys {
            let bucket = buckets.remove(&(domain.clone(), ptype)).unwrap_or_default();
            let total_failures: u64 = bucket.failures.values().sum();
            let mut failure_keys: Vec<_> = bucket.failures.keys().cloned().collect();
            failure_keys.sort_by(|a, b| {
                a.result_type
                    .as_str()
                    .cmp(b.result_type.as_str())
                    .then_with(|| a.receiving_mx_hostname.cmp(&b.receiving_mx_hostname))
            });
            let failure_details: Vec<FailureDetail> = failure_keys
                .into_iter()
                .map(|fk| {
                    let count = bucket.failures[&fk];
                    FailureDetail {
                        result_type: fk.result_type,
                        sending_mta_ip: fk.sending_mta_ip,
                        receiving_mx_hostname: fk.receiving_mx_hostname,
                        receiving_mx_helo: fk.receiving_mx_helo,
                        receiving_ip: fk.receiving_ip,
                        failed_session_count: count,
                        additional_information: fk.additional_information,
                        failure_reason_code: fk.failure_reason_code,
                    }
                })
                .collect();

            policies.push(PolicyReport {
                policy: PolicyBlock {
                    policy_type: ptype,
                    policy_string: bucket.policy_string,
                    policy_domain: domain,
                    mx_host: bucket.mx_hosts,
                },
                summary: SummaryBlock {
                    total_successful_session_count: bucket.success_count,
                    total_failure_session_count: total_failures,
                },
                failure_details,
            });
        }

        Ok(Report {
            organization_name,
            date_range,
            contact_info,
            report_id,
            policies,
        })
    }
}

impl Default for ReportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> ReportBuilder {
        ReportBuilder::new()
            .organization_name("Test Org")
            .contact_info("mailto:tlsrpt@example.com")
            .report_id("test-report-1")
            .date_range("2026-05-23T00:00:00Z", "2026-05-24T00:00:00Z")
    }

    #[test]
    fn empty_report_round_trips_through_json() {
        let r = fixture().build().unwrap();
        let json = serde_json::to_string_pretty(&r).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
        // RFC 8460 field names must appear verbatim.
        assert!(json.contains("\"organization-name\""));
        assert!(json.contains("\"date-range\""));
        assert!(json.contains("\"contact-info\""));
        assert!(json.contains("\"report-id\""));
    }

    #[test]
    fn build_rejects_missing_organization_name() {
        let r = ReportBuilder::new()
            .contact_info("mailto:x@y")
            .report_id("r")
            .date_range("a", "b")
            .build();
        assert!(matches!(
            r,
            Err(TlsRptError::MissingField("organization_name"))
        ));
    }

    #[test]
    fn build_rejects_missing_date_range() {
        let r = ReportBuilder::new()
            .organization_name("X")
            .contact_info("mailto:x@y")
            .report_id("r")
            .build();
        assert!(matches!(r, Err(TlsRptError::MissingField("date_range"))));
    }

    #[test]
    fn record_success_accumulates_count_and_mx_hosts() {
        let mut b = fixture();
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(),
        });
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "backup.example.com".into(),
        });
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(), // duplicate
        });
        let r = b.build().unwrap();
        assert_eq!(r.policies.len(), 1);
        let p = &r.policies[0];
        assert_eq!(p.summary.total_successful_session_count, 3);
        assert_eq!(p.summary.total_failure_session_count, 0);
        assert_eq!(p.policy.mx_host.len(), 2); // deduped
    }

    #[test]
    fn record_failure_buckets_by_context() {
        let mut b = fixture();
        // Three failures, two distinct contexts (same result-type +
        // mx hostname → one bucket; different result-type → second).
        b.record_failure(FailureEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: Some("mail.example.com".into()),
            result_type: FailureType::CertificateExpired,
            sending_mta_ip: None,
            receiving_ip: None,
            receiving_mx_helo: None,
            additional_information: None,
            failure_reason_code: None,
        });
        b.record_failure(FailureEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: Some("mail.example.com".into()),
            result_type: FailureType::CertificateExpired,
            sending_mta_ip: None,
            receiving_ip: None,
            receiving_mx_helo: None,
            additional_information: None,
            failure_reason_code: None,
        });
        b.record_failure(FailureEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: Some("mail.example.com".into()),
            result_type: FailureType::StarttlsNotSupported,
            sending_mta_ip: None,
            receiving_ip: None,
            receiving_mx_helo: None,
            additional_information: None,
            failure_reason_code: None,
        });
        let r = b.build().unwrap();
        assert_eq!(r.policies.len(), 1);
        let p = &r.policies[0];
        assert_eq!(p.summary.total_failure_session_count, 3);
        assert_eq!(p.failure_details.len(), 2);
        // The two CertificateExpired collapse into one row with count=2.
        let cert_row = p
            .failure_details
            .iter()
            .find(|f| f.result_type == FailureType::CertificateExpired)
            .unwrap();
        assert_eq!(cert_row.failed_session_count, 2);
        let starttls_row = p
            .failure_details
            .iter()
            .find(|f| f.result_type == FailureType::StarttlsNotSupported)
            .unwrap();
        assert_eq!(starttls_row.failed_session_count, 1);
    }

    #[test]
    fn buckets_split_by_distinct_policy_type() {
        let mut b = fixture();
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(),
        });
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Tlsa,
            mx_host: "mail.example.com".into(),
        });
        let r = b.build().unwrap();
        assert_eq!(r.policies.len(), 2);
    }

    #[test]
    fn policy_string_is_attached_to_bucket() {
        let lines = vec![
            "version: STSv1".to_string(),
            "mode: enforce".to_string(),
            "mx: mail.example.com".to_string(),
            "max_age: 604800".to_string(),
        ];
        let mut b = fixture().policy_string("example.com", PolicyType::Sts, lines.clone());
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(),
        });
        let r = b.build().unwrap();
        assert_eq!(r.policies[0].policy.policy_string, lines);
    }

    #[test]
    fn build_produces_stable_bucket_order() {
        // Bucket sort is (domain, policy_type). Two domains added in
        // reverse order must still emit in alphabetical order.
        let mut b = fixture();
        b.record_success(SuccessEvent {
            policy_domain: "z.example".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mx.z.example".into(),
        });
        b.record_success(SuccessEvent {
            policy_domain: "a.example".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mx.a.example".into(),
        });
        let r = b.build().unwrap();
        assert_eq!(r.policies.len(), 2);
        assert_eq!(r.policies[0].policy.policy_domain, "a.example");
        assert_eq!(r.policies[1].policy.policy_domain, "z.example");
    }

    #[test]
    fn failure_details_omitted_when_empty() {
        let mut b = fixture();
        b.record_success(SuccessEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: "mail.example.com".into(),
        });
        let r = b.build().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        // skip_serializing_if = Vec::is_empty
        assert!(!json.contains("failure-details"));
    }

    #[test]
    fn json_matches_rfc_field_naming() {
        let mut b = fixture();
        b.record_failure(FailureEvent {
            policy_domain: "example.com".into(),
            policy_type: PolicyType::Sts,
            mx_host: Some("mail.example.com".into()),
            result_type: FailureType::CertificateHostMismatch,
            sending_mta_ip: Some("10.0.0.1".parse().unwrap()),
            receiving_ip: Some("203.0.113.5".parse().unwrap()),
            receiving_mx_helo: Some("mail.example.com".into()),
            additional_information: Some("CN=other.example".into()),
            failure_reason_code: Some("BAD_CERTIFICATE".into()),
        });
        let r = b.build().unwrap();
        let json = serde_json::to_string(&r).unwrap();
        // RFC-mandated field names that are dashes-not-underscores.
        for needle in [
            "\"result-type\"",
            "\"sending-mta-ip\"",
            "\"receiving-mx-hostname\"",
            "\"receiving-mx-helo\"",
            "\"receiving-ip\"",
            "\"failed-session-count\"",
            "\"additional-information\"",
            "\"failure-reason-code\"",
            "\"certificate-host-mismatch\"",
        ] {
            assert!(json.contains(needle), "missing {needle} in {json}");
        }
    }
}
