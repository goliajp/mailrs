//! `TlsRptObserver` — appends each outbound TLS attempt into a
//! `mailrs_tls_rpt::Store` (`PgTlsRptStore` in production).

use std::sync::Arc;

use mailrs_outbound_queue::TlsAttemptOutcome;
#[cfg(test)]
use mailrs_tls_rpt::InMemoryStore;
use mailrs_tls_rpt::{
    EventFact, FailureEvent, FailureType, PolicyType, Report, ReportBuilder, Store, StoreError,
    SuccessEvent,
};

use super::convert::{policy_str_to_type, tls_outcome_to_failure_type, truncate};

/// Observer for outbound TLS attempts. Wraps any
/// [`Store`](mailrs_tls_rpt::Store) — typically
/// [`PgTlsRptStore`] in production or
/// [`InMemoryStore`](mailrs_tls_rpt::InMemoryStore) for tests.
pub struct TlsRptObserver {
    store: Arc<dyn Store>,
}

impl TlsRptObserver {
    /// New observer backed by the supplied store.
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    /// Convenience: observer backed by an in-memory store.
    /// Useful for tests and pilots where persistence isn't needed.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryStore::new()))
    }

    /// Record one STARTTLS attempt. The structured
    /// [`TlsAttemptOutcome`] from outbound-queue 1.2 lets us produce
    /// an accurate `(PolicyType, FailureType?)` tuple without
    /// keyword-matching error strings.
    ///
    /// - `Success { policy }` → one success EventFact.
    /// - `NotAdvertised` → one failure EventFact with
    ///   `FailureType::StarttlsNotSupported`.
    /// - `Rejected { code, message }` → one failure EventFact with
    ///   `FailureType::ValidationFailure` (no exact RFC 8460
    ///   vocabulary for "STARTTLS rejected" — catch-all + stash
    ///   the rejection text in additional-information).
    /// - `HandshakeFailed(outcome)` → one failure EventFact whose
    ///   `result_type` is the direct mapping
    ///   `outcome.as_str()` → RFC 8460 §4.3.
    ///
    /// Store errors are logged but don't propagate — TLSRPT is
    /// best-effort, we don't want a store outage to break the
    /// outbound delivery loop.
    pub async fn record_tls_attempt(
        &self,
        domain: &str,
        mx_host: &str,
        outcome: &TlsAttemptOutcome,
    ) {
        let now_unix = chrono::Utc::now().timestamp().max(0) as u64;
        let event = match outcome {
            TlsAttemptOutcome::Success { policy } => EventFact::Success(SuccessEvent {
                policy_domain: domain.to_string(),
                policy_type: policy_str_to_type(policy),
                mx_host: mx_host.to_string(),
            }),
            TlsAttemptOutcome::NotAdvertised => EventFact::Failure(FailureEvent {
                policy_domain: domain.to_string(),
                policy_type: PolicyType::NoPolicyFound,
                mx_host: Some(mx_host.to_string()),
                result_type: FailureType::StarttlsNotSupported,
                sending_mta_ip: None,
                receiving_ip: None,
                receiving_mx_helo: None,
                additional_information: None,
                failure_reason_code: None,
            }),
            TlsAttemptOutcome::Rejected { code, message } => EventFact::Failure(FailureEvent {
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
            }),
            TlsAttemptOutcome::HandshakeFailed(tls_outcome) => EventFact::Failure(FailureEvent {
                policy_domain: domain.to_string(),
                policy_type: PolicyType::NoPolicyFound,
                mx_host: Some(mx_host.to_string()),
                result_type: tls_outcome_to_failure_type(tls_outcome),
                sending_mta_ip: None,
                receiving_ip: None,
                receiving_mx_helo: None,
                additional_information: Some(truncate(tls_outcome.detail(), 200)),
                failure_reason_code: None,
            }),
        };
        if let Err(e) = self.store.append(event, now_unix).await {
            tracing::warn!(
                event = "tls_rpt_store_append_failed",
                error = %e,
                domain = domain,
                mx_host = mx_host,
                "TLSRPT event lost — store append failed"
            );
        }
    }

    /// Drain all facts in `[start, end)` and build the matching
    /// [`Report`]. Returns `None` when the window is empty so the
    /// caller can skip submitting an empty report.
    ///
    /// "Drain" semantics: the underlying [`Store`] removes the
    /// returned rows in the same transaction. A crash after this
    /// returns but before the caller submits loses one window's
    /// worth of submission (the spec tolerates occasional gaps).
    #[allow(clippy::too_many_arguments)]
    pub async fn take_report(
        &self,
        start_unix_secs: u64,
        end_unix_secs: u64,
        organization_name: &str,
        contact_info: &str,
        report_id: &str,
        start_datetime: &str,
        end_datetime: &str,
    ) -> Result<Option<Report>, StoreError> {
        let facts = self
            .store
            .drain_window(start_unix_secs, end_unix_secs)
            .await?;
        if facts.is_empty() {
            return Ok(None);
        }
        let mut builder = ReportBuilder::new()
            .organization_name(organization_name)
            .contact_info(contact_info)
            .report_id(report_id)
            .date_range(start_datetime, end_datetime);
        for fact in &facts {
            fact.apply(&mut builder);
        }
        // build() can fail only if a required field is missing,
        // which we just set above — unwrap is safe but match anyway.
        match builder.build() {
            Ok(r) => Ok(Some(r)),
            Err(e) => {
                tracing::warn!(
                    event = "tls_rpt_report_build_failed",
                    error = %e,
                    "TLSRPT report build failed despite required fields set"
                );
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_outbound_queue::TlsAttemptOutcome;
    use mailrs_smtp_client::TlsOutcome;
    use mailrs_tls_rpt::FailureType;

    #[tokio::test]
    async fn observer_in_memory_records_and_drains() {
        let o = TlsRptObserver::in_memory();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::Success {
                policy: "opportunistic",
            },
        )
        .await;
        let r = o
            .take_report(0, u64::MAX / 2, "Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r.policies.len(), 1);
        assert_eq!(r.policies[0].summary.total_successful_session_count, 1);
    }

    #[tokio::test]
    async fn observer_take_report_returns_none_when_window_empty() {
        let o = TlsRptObserver::in_memory();
        let r = o
            .take_report(0, 1000, "Org", "mailto:t@e.com", "rid", "a", "b")
            .await
            .unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn observer_take_report_drains_so_second_call_returns_none() {
        let o = TlsRptObserver::in_memory();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::Success {
                policy: "opportunistic",
            },
        )
        .await;
        let _ = o
            .take_report(0, u64::MAX / 2, "Org", "x", "r1", "a", "b")
            .await
            .unwrap();
        let r2 = o
            .take_report(0, u64::MAX / 2, "Org", "x", "r2", "a", "b")
            .await
            .unwrap();
        assert!(r2.is_none());
    }

    #[tokio::test]
    async fn observer_failure_event_persists_failure_type() {
        let o = TlsRptObserver::in_memory();
        o.record_tls_attempt(
            "example.com",
            "mx.example.com",
            &TlsAttemptOutcome::HandshakeFailed(TlsOutcome::CertificateExpired(
                "NotAfter 2024".into(),
            )),
        )
        .await;
        let r = o
            .take_report(0, u64::MAX / 2, "Org", "x", "r", "a", "b")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            r.policies[0].failure_details[0].result_type,
            FailureType::CertificateExpired
        );
    }
}
