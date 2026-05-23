//! Server-side observer that feeds outbound delivery events into
//! per-domain `mailrs_tls_rpt::ReportBuilder`s. RFC 8460 SMTP TLS
//! Reporting.
//!
//! ## What this records
//!
//! Each `DeliveryEvent::Success { domain }` becomes one `SuccessEvent`
//! against the receiving domain's `(policy_type, mx_host)` bucket.
//! Each `DeliveryEvent::Failed { domain, error }` becomes one
//! `FailureEvent` classified by best-effort error-string matching.
//!
//! ## What this does NOT do (and the path forward)
//!
//! - **No real TLS-result classification.** The current
//!   [`DeliveryEvent::Failed`] carries an opaque error string from
//!   the SMTP client; we infer `FailureType` by keyword match
//!   (`"STARTTLS"` → `StarttlsNotSupported`, `"cert"` /
//!   `"certificate"` → `CertificateNotTrusted`, etc.). A proper
//!   wire would require `mailrs-smtp-client` to expose a
//!   structured `TlsOutcome` next to the error string.
//! - **No actual report submission.** Daily flush builds the
//!   `Report` struct and logs it as JSON at `info` level; a
//!   follow-up will lookup the receiving domain's
//!   `_smtp._tls.<domain>` TXT and either email or HTTPS POST the
//!   gzipped report to each `rua` endpoint.
//! - **No on-disk persistence between restarts.** The in-memory
//!   bucket map resets on server restart; events recorded in the
//!   current window before a crash are lost. Persistence is a
//!   follow-up — TLSRPT receivers tolerate occasional gaps.
//!
//! These limits are deliberate for the wire-C MVP: the observer
//! proves the data flow end-to-end and the report shape is correct
//! against real outbound traffic, without committing to the full
//! submission stack yet.

use std::sync::Arc;

use mailrs_tls_rpt::{
    FailureEvent, FailureType, PolicyType, ReportBuilder, SuccessEvent,
};
use tokio::sync::Mutex;

/// Builds and accumulates per-window TLSRPT reports.
#[derive(Default)]
pub struct TlsRptObserver {
    inner: Mutex<TlsRptInner>,
}

#[derive(Default)]
struct TlsRptInner {
    /// One builder per (reporting-window, domain). For the MVP we
    /// keep one builder per reporting-domain-bucket that resets on
    /// `take_report`. A future revision will key on
    /// (date, domain) to support multi-day batching.
    builder: ReportBuilder,
    /// Whether any events have been recorded since last
    /// `take_report` — lets the caller skip building empty reports.
    has_events: bool,
}

impl TlsRptObserver {
    /// New observer with a fresh empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful TLS-bearing SMTP delivery to `domain` via
    /// the named MX host.
    pub async fn record_success(&self, domain: &str, mx_host: &str) {
        let mut inner = self.inner.lock().await;
        // Note: PolicyType::NoPolicyFound covers the "opportunistic
        // STARTTLS without enforced MTA-STS/DANE" path that today's
        // outbound path uses. When we wire MTA-STS enforcement
        // through the queue, swap to PolicyType::Sts on enforced
        // paths.
        inner.builder.record_success(SuccessEvent {
            policy_domain: domain.to_string(),
            policy_type: PolicyType::NoPolicyFound,
            mx_host: mx_host.to_string(),
        });
        inner.has_events = true;
    }

    /// Record a failed delivery. `error` is the opaque SMTP-client
    /// error string; we classify it into a [`FailureType`] by
    /// keyword match.
    pub async fn record_failure(&self, domain: &str, mx_host: Option<&str>, error: &str) {
        let mut inner = self.inner.lock().await;
        let result_type = classify(error);
        inner.builder.record_failure(FailureEvent {
            policy_domain: domain.to_string(),
            policy_type: PolicyType::NoPolicyFound,
            mx_host: mx_host.map(|s| s.to_string()),
            result_type,
            sending_mta_ip: None,
            receiving_ip: None,
            receiving_mx_helo: None,
            additional_information: Some(truncate(error, 200)),
            failure_reason_code: None,
        });
        inner.has_events = true;
    }

    /// Build the accumulated report for the supplied window. Returns
    /// `None` if no events have been recorded — caller skips sending
    /// an empty report.
    ///
    /// After this call the internal builder is reset to a fresh empty
    /// state, ready to accumulate the next window's events.
    pub async fn take_report(
        &self,
        organization_name: &str,
        contact_info: &str,
        report_id: &str,
        start_datetime: &str,
        end_datetime: &str,
    ) -> Option<mailrs_tls_rpt::Report> {
        let mut inner = self.inner.lock().await;
        if !inner.has_events {
            return None;
        }
        let builder = std::mem::take(&mut inner.builder)
            .organization_name(organization_name)
            .contact_info(contact_info)
            .report_id(report_id)
            .date_range(start_datetime, end_datetime);
        inner.has_events = false;
        builder.build().ok()
    }
}

/// Classify an SMTP-client error string into the closest RFC 8460
/// §4.3 `FailureType`. Best-effort keyword match; the smtp-client
/// path doesn't yet emit structured TLS outcomes (see module-level
/// doc).
fn classify(error: &str) -> FailureType {
    let e = error.to_ascii_lowercase();
    if e.contains("starttls") {
        return FailureType::StarttlsNotSupported;
    }
    if e.contains("certificate") && e.contains("expired") {
        return FailureType::CertificateExpired;
    }
    if e.contains("certificate") && (e.contains("mismatch") || e.contains("hostname")) {
        return FailureType::CertificateHostMismatch;
    }
    if e.contains("untrusted") || e.contains("unknown ca") || e.contains("unknown authority") {
        return FailureType::CertificateNotTrusted;
    }
    if e.contains("dnssec") {
        return FailureType::DnssecInvalid;
    }
    if e.contains("dane") {
        return FailureType::DaneRequired;
    }
    if e.contains("mta-sts") || e.contains("sts policy") {
        return FailureType::StsPolicyFetchError;
    }
    FailureType::ValidationFailure
}

/// Truncate a string to at most `n` chars, preserving char boundaries.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect()
}

/// Convenience: build an Arc<TlsRptObserver> for sharing across
/// the spawned outbound-event handler + the periodic flush task.
pub fn new_shared() -> Arc<TlsRptObserver> {
    Arc::new(TlsRptObserver::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_starttls_not_supported() {
        let r = classify("STARTTLS extension not advertised");
        assert_eq!(r, FailureType::StarttlsNotSupported);
    }

    #[test]
    fn classify_certificate_expired() {
        let r = classify("certificate expired (NotAfter 2024)");
        assert_eq!(r, FailureType::CertificateExpired);
    }

    #[test]
    fn classify_certificate_hostname_mismatch() {
        let r = classify("certificate hostname mismatch");
        assert_eq!(r, FailureType::CertificateHostMismatch);
    }

    #[test]
    fn classify_falls_back_to_validation_failure() {
        let r = classify("some random connection error");
        assert_eq!(r, FailureType::ValidationFailure);
    }

    #[test]
    fn truncate_keeps_char_boundaries() {
        // Multi-byte chars must not be cut mid-byte
        let s = "日本語テストstring";
        let t = truncate(s, 5);
        assert_eq!(t.chars().count(), 5);
    }

    #[tokio::test]
    async fn take_report_returns_none_when_no_events() {
        let o = TlsRptObserver::new();
        let r = o.take_report(
            "Org",
            "mailto:t@e.com",
            "rid",
            "2026-05-23T00:00:00Z",
            "2026-05-24T00:00:00Z",
        )
        .await;
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn take_report_returns_built_report_after_events() {
        let o = TlsRptObserver::new();
        o.record_success("example.com", "mx.example.com").await;
        o.record_failure("example.com", Some("mx.example.com"), "STARTTLS not offered")
            .await;
        let r = o.take_report(
            "Org",
            "mailto:t@e.com",
            "rid",
            "2026-05-23T00:00:00Z",
            "2026-05-24T00:00:00Z",
        )
        .await;
        let r = r.expect("report built");
        assert_eq!(r.policies.len(), 1);
        assert_eq!(r.policies[0].summary.total_successful_session_count, 1);
        assert_eq!(r.policies[0].summary.total_failure_session_count, 1);
        assert_eq!(r.policies[0].failure_details.len(), 1);
        assert_eq!(
            r.policies[0].failure_details[0].result_type,
            FailureType::StarttlsNotSupported
        );
    }

    #[tokio::test]
    async fn take_report_resets_builder_after_call() {
        let o = TlsRptObserver::new();
        o.record_success("example.com", "mx.example.com").await;
        let _ = o.take_report("Org", "x", "r1", "a", "b").await;
        let r2 = o.take_report("Org", "x", "r2", "a", "b").await;
        assert!(r2.is_none(), "builder should be empty after take_report");
    }
}
