//! Server-side observer that feeds outbound delivery events into
//! per-domain `mailrs_tls_rpt::ReportBuilder`s. RFC 8460 SMTP TLS
//! Reporting.
//!
//! ## What this records
//!
//! Since the outbound-queue 1.2 / smtp-client 1.1 cut, classification
//! comes from a structured [`mailrs_outbound_queue::TlsAttemptOutcome`]
//! carried on [`mailrs_outbound_queue::DeliveryEvent::TlsAttempt`] —
//! `record_tls_attempt` maps each variant directly to its RFC 8460
//! §4.3 FailureType. No more keyword-classifying error strings.
//!
//! ## What this does NOT do (and the path forward)
//!
//! - **No actual report submission.** Daily flush builds the
//!   `Report` struct and logs it as JSON at `info` level; a
//!   follow-up will lookup the receiving domain's
//!   `_smtp._tls.<domain>` TXT and either email or HTTPS POST the
//!   gzipped report to each `rua` endpoint.
//! - **No on-disk persistence between restarts.** The in-memory
//!   bucket map resets on server restart; events recorded in the
//!   current window before a crash are lost. Persistence is a
//!   follow-up — TLSRPT receivers tolerate occasional gaps.

use std::sync::Arc;

use mailrs_outbound_queue::TlsAttemptOutcome;
use mailrs_smtp_client::TlsOutcome;
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

    /// Record one STARTTLS attempt. The structured
    /// [`TlsAttemptOutcome`] from outbound-queue 1.2 lets us produce
    /// an accurate `(PolicyType, FailureType?)` tuple without
    /// keyword-matching error strings.
    ///
    /// - `Success { policy }` → one [`SuccessEvent`] in the matching
    ///   `(domain, PolicyType)` bucket. `policy` is one of
    ///   `"dane"` / `"sts"` / `"opportunistic"`; we map to
    ///   `PolicyType::{Tlsa, Sts, NoPolicyFound}`.
    /// - `NotAdvertised` → one [`FailureEvent`] with
    ///   `FailureType::StarttlsNotSupported`.
    /// - `Rejected { code, message }` → one [`FailureEvent`] with
    ///   `FailureType::ValidationFailure` (the RFC 8460 vocabulary
    ///   doesn't have a "STARTTLS rejected" category, so we use the
    ///   catch-all and stash the rejection text in
    ///   `additional-information`).
    /// - `HandshakeFailed(outcome)` → one [`FailureEvent`] whose
    ///   `result_type` comes from the direct mapping
    ///   `outcome.as_str()` → RFC 8460 §4.3 string.
    pub async fn record_tls_attempt(
        &self,
        domain: &str,
        mx_host: &str,
        outcome: &TlsAttemptOutcome,
    ) {
        let mut inner = self.inner.lock().await;
        match outcome {
            TlsAttemptOutcome::Success { policy } => {
                let policy_type = policy_str_to_type(policy);
                inner.builder.record_success(SuccessEvent {
                    policy_domain: domain.to_string(),
                    policy_type,
                    mx_host: mx_host.to_string(),
                });
            }
            TlsAttemptOutcome::NotAdvertised => {
                inner.builder.record_failure(FailureEvent {
                    policy_domain: domain.to_string(),
                    policy_type: PolicyType::NoPolicyFound,
                    mx_host: Some(mx_host.to_string()),
                    result_type: FailureType::StarttlsNotSupported,
                    sending_mta_ip: None,
                    receiving_ip: None,
                    receiving_mx_helo: None,
                    additional_information: None,
                    failure_reason_code: None,
                });
            }
            TlsAttemptOutcome::Rejected { code, message } => {
                inner.builder.record_failure(FailureEvent {
                    policy_domain: domain.to_string(),
                    policy_type: PolicyType::NoPolicyFound,
                    mx_host: Some(mx_host.to_string()),
                    result_type: FailureType::ValidationFailure,
                    sending_mta_ip: None,
                    receiving_ip: None,
                    receiving_mx_helo: None,
                    additional_information: Some(truncate(
                        &format!("STARTTLS rejected {code}: {message}"),
                        200,
                    )),
                    failure_reason_code: Some(code.to_string()),
                });
            }
            TlsAttemptOutcome::HandshakeFailed(tls_outcome) => {
                let result_type = tls_outcome_to_failure_type(tls_outcome);
                inner.builder.record_failure(FailureEvent {
                    policy_domain: domain.to_string(),
                    policy_type: PolicyType::NoPolicyFound,
                    mx_host: Some(mx_host.to_string()),
                    result_type,
                    sending_mta_ip: None,
                    receiving_ip: None,
                    receiving_mx_helo: None,
                    additional_information: Some(truncate(tls_outcome.detail(), 200)),
                    failure_reason_code: None,
                });
            }
        }
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

/// Map `mailrs_smtp_client::TlsOutcome` directly into the RFC 8460
/// §4.3 `FailureType`. 1:1 mapping; no string matching involved.
fn tls_outcome_to_failure_type(o: &TlsOutcome) -> FailureType {
    match o {
        TlsOutcome::CertificateExpired(_) => FailureType::CertificateExpired,
        TlsOutcome::CertificateHostMismatch(_) => FailureType::CertificateHostMismatch,
        TlsOutcome::CertificateNotTrusted(_) => FailureType::CertificateNotTrusted,
        TlsOutcome::DaneValidationFailure(_) => FailureType::TlsaInvalid,
        // The next three don't have a clean RFC 8460 mapping (the spec
        // assumes the failure is on the receiving side). ValidationFailure
        // is the catch-all per §4.3.
        TlsOutcome::InvalidServerName(_)
        | TlsOutcome::NetworkError(_)
        | TlsOutcome::Other(_) => FailureType::ValidationFailure,
    }
}

/// Map outbound-queue's policy hint string into the report's
/// `PolicyType`. Unknown / future values fall back to
/// `NoPolicyFound` so we don't lose the event.
fn policy_str_to_type(s: &str) -> PolicyType {
    match s {
        "dane" => PolicyType::Tlsa,
        "sts" => PolicyType::Sts,
        _ => PolicyType::NoPolicyFound,
    }
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
    fn outcome_mapping_certificate_expired() {
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::CertificateExpired("x".into())),
            FailureType::CertificateExpired
        );
    }

    #[test]
    fn outcome_mapping_host_mismatch() {
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::CertificateHostMismatch("x".into())),
            FailureType::CertificateHostMismatch
        );
    }

    #[test]
    fn outcome_mapping_dane_failure_to_tlsa_invalid() {
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::DaneValidationFailure("x".into())),
            FailureType::TlsaInvalid
        );
    }

    #[test]
    fn outcome_mapping_other_falls_back_to_validation_failure() {
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::Other("x".into())),
            FailureType::ValidationFailure
        );
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::NetworkError("x".into())),
            FailureType::ValidationFailure
        );
        assert_eq!(
            tls_outcome_to_failure_type(&TlsOutcome::InvalidServerName("x".into())),
            FailureType::ValidationFailure
        );
    }

    #[test]
    fn policy_str_dane_to_tlsa() {
        assert_eq!(policy_str_to_type("dane"), PolicyType::Tlsa);
    }

    #[test]
    fn policy_str_sts_to_sts() {
        assert_eq!(policy_str_to_type("sts"), PolicyType::Sts);
    }

    #[test]
    fn policy_str_opportunistic_to_no_policy_found() {
        assert_eq!(policy_str_to_type("opportunistic"), PolicyType::NoPolicyFound);
    }

    #[test]
    fn policy_str_unknown_to_no_policy_found() {
        assert_eq!(policy_str_to_type("future-policy-name"), PolicyType::NoPolicyFound);
    }

    #[test]
    fn truncate_keeps_char_boundaries() {
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
    async fn record_tls_attempt_success_records_one_success() {
        let o = TlsRptObserver::new();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::Success {
                policy: "opportunistic",
            },
        )
        .await;
        let r = o
            .take_report("Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap();
        assert_eq!(r.policies.len(), 1);
        assert_eq!(r.policies[0].summary.total_successful_session_count, 1);
        assert_eq!(r.policies[0].summary.total_failure_session_count, 0);
        assert_eq!(r.policies[0].policy.policy_type, PolicyType::NoPolicyFound);
    }

    #[tokio::test]
    async fn record_tls_attempt_dane_success_uses_tlsa_policy() {
        let o = TlsRptObserver::new();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::Success { policy: "dane" },
        )
        .await;
        let r = o
            .take_report("Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap();
        assert_eq!(r.policies[0].policy.policy_type, PolicyType::Tlsa);
    }

    #[tokio::test]
    async fn record_tls_attempt_not_advertised_emits_starttls_not_supported() {
        let o = TlsRptObserver::new();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::NotAdvertised,
        )
        .await;
        let r = o
            .take_report("Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap();
        assert_eq!(
            r.policies[0].failure_details[0].result_type,
            FailureType::StarttlsNotSupported
        );
    }

    #[tokio::test]
    async fn record_tls_attempt_handshake_failed_maps_outcome() {
        let o = TlsRptObserver::new();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::HandshakeFailed(TlsOutcome::CertificateExpired(
                "NotAfter 2024".into(),
            )),
        )
        .await;
        let r = o
            .take_report("Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap();
        assert_eq!(
            r.policies[0].failure_details[0].result_type,
            FailureType::CertificateExpired
        );
    }

    #[tokio::test]
    async fn take_report_resets_builder_after_call() {
        let o = TlsRptObserver::new();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::Success {
                policy: "opportunistic",
            },
        )
        .await;
        let _ = o.take_report("Org", "x", "r1", "a", "b").await;
        let r2 = o.take_report("Org", "x", "r2", "a", "b").await;
        assert!(r2.is_none(), "builder should be empty after take_report");
    }
}
