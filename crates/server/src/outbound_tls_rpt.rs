//! Server-side observer that feeds outbound delivery events into
//! a persistent TLSRPT [`Store`]. RFC 8460 SMTP TLS Reporting.
//!
//! ## What this records
//!
//! Each `mailrs_outbound_queue::DeliveryEvent::TlsAttempt` becomes
//! one [`EventFact`] appended to the store with the current
//! recorded-at timestamp. Classification comes from the structured
//! [`mailrs_outbound_queue::TlsAttemptOutcome`] — no error-string
//! keyword matching anywhere in this path.
//!
//! ## Persistence
//!
//! Since 1.x, the observer is just a thin facade over a
//! [`mailrs_tls_rpt::Store`]. The `PgTlsRptStore` impl writes
//! every event into `tls_rpt_events` (an append-only PG table —
//! see `scripts/migrate-036-tls-rpt-events.sql`). Daily flush
//! drains the window, rebuilds the report, and submits.
//!
//! Restart-safe: events recorded before a crash survive in PG and
//! the next flush picks them up. The in-process observer carries
//! no state of its own.
//!
//! ## Submission
//!
//! [`submit_report`] performs the per-policy-domain submission:
//! lookup `_smtp._tls.<domain>` TXT, parse into a
//! [`mailrs_tls_rpt::TlsRptRecord`], then for each `rua` endpoint
//! either enqueue an outbound email (mailto:) or HTTPS POST the
//! gzipped report (https:). Per-endpoint failures are logged but
//! don't abort other endpoints' submission.

use std::sync::Arc;

use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use mailrs_outbound_queue::TlsAttemptOutcome;
use mailrs_smtp_client::TlsOutcome;
use mailrs_tls_rpt::{
    EventFact, FailureEvent, FailureType, PolicyType, Report, ReportBuilder, RuaEndpoint, Store,
    StoreError, SubmissionEmailOpts, SuccessEvent, TlsRptRecord, build_submission_email,
    gzip_report,
};
#[cfg(test)]
use mailrs_tls_rpt::InMemoryStore;
use sqlx::PgPool;

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

/// Postgres-backed [`Store`] implementation. Append goes into the
/// `tls_rpt_events` table (see scripts/migrate-036-tls-rpt-events.sql);
/// drain `SELECT … WHERE recorded_at_unix >= $1 AND < $2 RETURNING *`
/// against `DELETE` in one transaction.
pub struct PgTlsRptStore {
    pool: PgPool,
}

impl PgTlsRptStore {
    /// New store wrapping the supplied PG pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convenience: wrap into an `Arc<dyn Store>` ready to hand to
    /// [`TlsRptObserver::new`].
    pub fn into_arc(self) -> Arc<dyn Store> {
        Arc::new(self)
    }
}

#[async_trait]
impl Store for PgTlsRptStore {
    async fn append(
        &self,
        event: EventFact,
        recorded_at_unix_secs: u64,
    ) -> Result<(), StoreError> {
        let ts = recorded_at_unix_secs as i64;
        let r = match event {
            EventFact::Success(e) => sqlx::query(
                "INSERT INTO tls_rpt_events
                    (recorded_at_unix, kind, policy_domain, policy_type, mx_host)
                 VALUES ($1, 'success', $2, $3, $4)",
            )
            .bind(ts)
            .bind(e.policy_domain)
            .bind(policy_type_str(e.policy_type))
            .bind(e.mx_host)
            .execute(&self.pool)
            .await,
            EventFact::Failure(e) => sqlx::query(
                "INSERT INTO tls_rpt_events
                    (recorded_at_unix, kind, policy_domain, policy_type, mx_host,
                     result_type, sending_mta_ip, receiving_ip, receiving_mx_helo,
                     additional_information, failure_reason_code)
                 VALUES ($1, 'failure', $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(ts)
            .bind(e.policy_domain)
            .bind(policy_type_str(e.policy_type))
            .bind(e.mx_host)
            .bind(failure_type_str(e.result_type))
            .bind(e.sending_mta_ip.map(|ip| ip.to_string()))
            .bind(e.receiving_ip.map(|ip| ip.to_string()))
            .bind(e.receiving_mx_helo)
            .bind(e.additional_information)
            .bind(e.failure_reason_code)
            .execute(&self.pool)
            .await,
        };
        r.map(|_| ()).map_err(|e| StoreError::Backend(Box::new(e)))
    }

    async fn drain_window(
        &self,
        start_unix_secs: u64,
        end_unix_secs: u64,
    ) -> Result<Vec<EventFact>, StoreError> {
        let start = start_unix_secs as i64;
        let end = end_unix_secs as i64;
        // DELETE ... RETURNING is the atomic drain — selecting +
        // deleting in two separate statements would leave a window
        // for a concurrent reader to see stale rows. PG's
        // RETURNING clause guarantees the deleted rows are
        // captured before they disappear from any other view.
        let rows: Vec<TlsRptRow> = sqlx::query_as::<_, TlsRptRow>(
            "DELETE FROM tls_rpt_events
             WHERE recorded_at_unix >= $1 AND recorded_at_unix < $2
             RETURNING kind, policy_domain, policy_type, mx_host,
                       result_type, sending_mta_ip, receiving_ip,
                       receiving_mx_helo, additional_information,
                       failure_reason_code",
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::Backend(Box::new(e)))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let policy_type = canonical_policy_str_to_type(&row.policy_type);
            if row.kind == "success" {
                let mx = row.mx_host.unwrap_or_default();
                out.push(EventFact::Success(SuccessEvent {
                    policy_domain: row.policy_domain,
                    policy_type,
                    mx_host: mx,
                }));
            } else {
                let result_type = str_to_failure_type(row.result_type.as_deref().unwrap_or(""));
                out.push(EventFact::Failure(FailureEvent {
                    policy_domain: row.policy_domain,
                    policy_type,
                    mx_host: row.mx_host,
                    result_type,
                    sending_mta_ip: row.sending_mta_ip.and_then(|s| s.parse().ok()),
                    receiving_ip: row.receiving_ip.and_then(|s| s.parse().ok()),
                    receiving_mx_helo: row.receiving_mx_helo,
                    additional_information: row.additional_information,
                    failure_reason_code: row.failure_reason_code,
                }));
            }
        }
        Ok(out)
    }
}

#[derive(sqlx::FromRow)]
struct TlsRptRow {
    kind: String,
    policy_domain: String,
    policy_type: String,
    mx_host: Option<String>,
    result_type: Option<String>,
    sending_mta_ip: Option<String>,
    receiving_ip: Option<String>,
    receiving_mx_helo: Option<String>,
    additional_information: Option<String>,
    failure_reason_code: Option<String>,
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
        TlsOutcome::InvalidServerName(_)
        | TlsOutcome::NetworkError(_)
        | TlsOutcome::Other(_) => FailureType::ValidationFailure,
    }
}

/// Map outbound-queue's policy HINT string (used by the worker
/// when emitting `TlsAttempt`) into the report's `PolicyType`.
/// The worker uses "dane" / "sts" / "opportunistic" — these are
/// the human-readable hint values, NOT the canonical RFC 8460
/// `policy-type` strings.
fn policy_str_to_type(s: &str) -> PolicyType {
    match s {
        "dane" => PolicyType::Tlsa,
        "sts" => PolicyType::Sts,
        _ => PolicyType::NoPolicyFound,
    }
}

/// Canonical RFC 8460 §4.2 `policy-type` string. Used for
/// round-trip storage in the PG `tls_rpt_events.policy_type`
/// column so the DB rows match the report's wire format exactly.
fn policy_type_str(p: PolicyType) -> &'static str {
    match p {
        PolicyType::Sts => "sts",
        PolicyType::Tlsa => "tlsa",
        PolicyType::NoPolicyFound => "no-policy-found",
    }
}

/// Inverse of [`policy_type_str`] — parses the canonical RFC 8460
/// strings stored in the PG table. Distinct from
/// [`policy_str_to_type`] because the worker's hint vocabulary
/// uses "dane" but the canonical vocabulary uses "tlsa"; conflating
/// them would silently corrupt round-trips.
fn canonical_policy_str_to_type(s: &str) -> PolicyType {
    match s {
        "sts" => PolicyType::Sts,
        "tlsa" => PolicyType::Tlsa,
        _ => PolicyType::NoPolicyFound,
    }
}

fn failure_type_str(f: FailureType) -> &'static str {
    f.as_str()
}

fn str_to_failure_type(s: &str) -> FailureType {
    match s {
        "starttls-not-supported" => FailureType::StarttlsNotSupported,
        "certificate-host-mismatch" => FailureType::CertificateHostMismatch,
        "certificate-expired" => FailureType::CertificateExpired,
        "certificate-not-trusted" => FailureType::CertificateNotTrusted,
        "validation-failure" => FailureType::ValidationFailure,
        "sts-policy-fetch-error" => FailureType::StsPolicyFetchError,
        "sts-policy-invalid" => FailureType::StsPolicyInvalid,
        "sts-webpki-invalid" => FailureType::StsWebpkiInvalid,
        "tlsa-invalid" => FailureType::TlsaInvalid,
        "dnssec-invalid" => FailureType::DnssecInvalid,
        "dane-required" => FailureType::DaneRequired,
        "dnssec-not-supported" => FailureType::DnssecNotSupported,
        "mx-mismatch" => FailureType::MxMismatch,
        "policy-not-published" => FailureType::PolicyNotPublished,
        _ => FailureType::ValidationFailure,
    }
}

/// Truncate a string to at most `n` chars, preserving char boundaries.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect()
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
    }

    #[test]
    fn canonical_policy_str_round_trips_through_db_serialization() {
        for p in [PolicyType::Sts, PolicyType::Tlsa, PolicyType::NoPolicyFound] {
            let s = policy_type_str(p);
            assert_eq!(canonical_policy_str_to_type(s), p);
        }
    }

    #[test]
    fn worker_hint_dane_maps_to_tlsa() {
        // Worker emits "dane" as its policy hint; we map to
        // PolicyType::Tlsa (the canonical RFC 8460 name).
        assert_eq!(policy_str_to_type("dane"), PolicyType::Tlsa);
        assert_eq!(policy_str_to_type("sts"), PolicyType::Sts);
        assert_eq!(policy_str_to_type("opportunistic"), PolicyType::NoPolicyFound);
    }

    #[test]
    fn failure_type_str_round_trip() {
        // Spot-check every variant round-trips back through
        // str_to_failure_type — guards against drift between
        // `FailureType::as_str` and our DB-row parser.
        for f in [
            FailureType::StarttlsNotSupported,
            FailureType::CertificateHostMismatch,
            FailureType::CertificateExpired,
            FailureType::CertificateNotTrusted,
            FailureType::ValidationFailure,
            FailureType::StsPolicyFetchError,
            FailureType::StsPolicyInvalid,
            FailureType::StsWebpkiInvalid,
            FailureType::TlsaInvalid,
            FailureType::DnssecInvalid,
            FailureType::DaneRequired,
            FailureType::DnssecNotSupported,
            FailureType::MxMismatch,
            FailureType::PolicyNotPublished,
        ] {
            assert_eq!(str_to_failure_type(failure_type_str(f)), f);
        }
    }

    #[test]
    fn truncate_keeps_char_boundaries() {
        let s = "日本語テストstring";
        let t = truncate(s, 5);
        assert_eq!(t.chars().count(), 5);
    }

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
            .take_report(
                0,
                u64::MAX / 2,
                "Org",
                "mailto:t@e.com",
                "rid",
                "a",
                "b",
            )
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
