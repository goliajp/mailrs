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
//! ## Submission
//!
//! [`submit_report`] performs the per-policy-domain submission:
//! lookup `_smtp._tls.<domain>` TXT, parse into a
//! [`mailrs_tls_rpt::TlsRptRecord`], then for each `rua` endpoint
//! either enqueue an outbound email (mailto:) or HTTPS POST the
//! gzipped report (https:). Per-endpoint failures are logged but
//! don't abort other endpoints' submission.
//!
//! ## What this does NOT do (and the path forward)
//!
//! - **No on-disk persistence between restarts.** The in-memory
//!   bucket map resets on server restart; events recorded in the
//!   current window before a crash are lost. Persistence is a
//!   follow-up — TLSRPT receivers tolerate occasional gaps.

use std::sync::Arc;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mailrs_outbound_queue::TlsAttemptOutcome;
use mailrs_smtp_client::TlsOutcome;
use mailrs_tls_rpt::{
    FailureEvent, FailureType, PolicyType, Report, ReportBuilder, RuaEndpoint,
    SubmissionEmailOpts, SuccessEvent, TlsRptRecord, build_submission_email, gzip_report,
};
use sqlx::PgPool;
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

/// Submit one [`Report`] to all `rua` endpoints declared by each
/// policy-domain it covers.
///
/// For each domain in `report.policies`, lookup
/// `_smtp._tls.<domain>` TXT via `resolver`. Parse with
/// [`TlsRptRecord::parse`]. For each `rua` endpoint:
///
/// - **mailto:** build a §5.3-compliant `multipart/report` email
///   via [`build_submission_email`] and enqueue it into the
///   outbound queue. The outbound queue's existing DKIM-sign
///   pipeline takes care of the RFC-8460-required signature on
///   submission emails.
/// - **https:** POST the gzipped JSON with
///   `Content-Type: application/tlsrpt+gzip`. Per RFC 8460 §6,
///   2xx is success; anything else is logged as a per-endpoint
///   failure but doesn't abort other endpoints.
///
/// Per-endpoint failures are logged at `warn` and the function
/// continues — TLSRPT is best-effort. The function returns
/// `(successful_endpoints, failed_endpoints)` for the caller's
/// metrics.
pub async fn submit_report(
    report: &Report,
    submitter_domain: &str,
    submitter_address: &str,
    resolver: &TokioResolver,
    outbound_pool: Option<&PgPool>,
    http_client: Option<&reqwest::Client>,
) -> (usize, usize) {
    let mut ok = 0usize;
    let mut failed = 0usize;
    let gzipped = match gzip_report(report) {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(
                event = "tls_rpt_gzip_failed",
                error = %e,
                "TLSRPT submission aborted — gzip step failed"
            );
            return (0, report.policies.len());
        }
    };
    let date_rfc2822 = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S +0000")
        .to_string();

    for policy in &report.policies {
        let receiving_domain = policy.policy.policy_domain.as_str();
        let q = format!("_smtp._tls.{receiving_domain}");
        let lookup = match resolver.txt_lookup(&q).await {
            Ok(r) => r,
            Err(e) => {
                tracing::info!(
                    event = "tls_rpt_no_record",
                    domain = receiving_domain,
                    error = %e,
                    "_smtp._tls TXT lookup failed — receiving domain doesn't publish TLSRPT"
                );
                continue;
            }
        };
        let txt: String = lookup
            .answers()
            .iter()
            .filter_map(|r| match &r.data {
                RData::TXT(t) => Some(t.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let record = match TlsRptRecord::parse(&txt) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    event = "tls_rpt_record_parse_failed",
                    domain = receiving_domain,
                    error = %e,
                    "TLSRPT record at {q} failed to parse — skipping submission"
                );
                continue;
            }
        };

        for endpoint in &record.rua {
            match endpoint {
                RuaEndpoint::Mailto(addr) => {
                    let Some(pool) = outbound_pool else {
                        tracing::warn!(
                            event = "tls_rpt_mailto_no_queue",
                            domain = receiving_domain,
                            endpoint = %addr,
                            "TLSRPT mailto: submission requires an outbound queue; skipping"
                        );
                        failed += 1;
                        continue;
                    };
                    let boundary = format!(
                        "tlsrpt-{}-{}",
                        report.report_id.replace(['<', '>', '@'], "-"),
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
                    );
                    let opts = SubmissionEmailOpts {
                        from_address: submitter_address.to_string(),
                        to_address: addr.clone(),
                        receiving_domain: receiving_domain.to_string(),
                        submitter_domain: submitter_domain.to_string(),
                        report_id: report.report_id.clone(),
                        date_rfc2822: date_rfc2822.clone(),
                        boundary,
                        report_gzipped: gzipped.clone(),
                    };
                    let email = build_submission_email(&opts);
                    let recipient_domain = addr.rsplit_once('@').map(|(_, d)| d).unwrap_or(addr);
                    let now = chrono::Utc::now().timestamp();
                    match mailrs_outbound_queue::queue::enqueue(
                        pool,
                        submitter_address,
                        addr,
                        recipient_domain,
                        &email,
                        Some(&report.report_id),
                        now,
                    )
                    .await
                    {
                        Ok(queue_id) => {
                            tracing::info!(
                                event = "tls_rpt_mailto_enqueued",
                                domain = receiving_domain,
                                endpoint = %addr,
                                queue_id = queue_id,
                                "TLSRPT mailto report enqueued"
                            );
                            ok += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                event = "tls_rpt_mailto_enqueue_failed",
                                domain = receiving_domain,
                                endpoint = %addr,
                                error = %e,
                                "TLSRPT mailto enqueue failed"
                            );
                            failed += 1;
                        }
                    }
                }
                RuaEndpoint::Https(url) => {
                    let Some(client) = http_client else {
                        tracing::warn!(
                            event = "tls_rpt_https_no_client",
                            domain = receiving_domain,
                            endpoint = %url,
                            "TLSRPT https: submission requires a reqwest client; skipping"
                        );
                        failed += 1;
                        continue;
                    };
                    match client
                        .post(url)
                        .header("Content-Type", "application/tlsrpt+gzip")
                        .body(gzipped.clone())
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            tracing::info!(
                                event = "tls_rpt_https_submitted",
                                domain = receiving_domain,
                                endpoint = %url,
                                status = %resp.status(),
                                "TLSRPT https report submitted"
                            );
                            ok += 1;
                        }
                        Ok(resp) => {
                            tracing::warn!(
                                event = "tls_rpt_https_non_2xx",
                                domain = receiving_domain,
                                endpoint = %url,
                                status = %resp.status(),
                                "TLSRPT https endpoint returned non-2xx"
                            );
                            failed += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                event = "tls_rpt_https_send_failed",
                                domain = receiving_domain,
                                endpoint = %url,
                                error = %e,
                                "TLSRPT https POST failed"
                            );
                            failed += 1;
                        }
                    }
                }
            }
        }
    }
    (ok, failed)
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
