//! Tests for `outbound_tls_rpt` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

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
