//! Type conversions between the structured
//! `mailrs_outbound_queue::TlsAttemptOutcome` / `mailrs_smtp_client::TlsOutcome`
//! enums and the wire-format `FailureType` / `PolicyType` strings
//! used by RFC 8460 reports + the PG store.

use mailrs_smtp_client::TlsOutcome;
use mailrs_tls_rpt::{FailureType, PolicyType};

pub(super) fn tls_outcome_to_failure_type(o: &TlsOutcome) -> FailureType {
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
pub(super) fn policy_str_to_type(s: &str) -> PolicyType {
    match s {
        "dane" => PolicyType::Tlsa,
        "sts" => PolicyType::Sts,
        _ => PolicyType::NoPolicyFound,
    }
}

/// Canonical RFC 8460 §4.2 `policy-type` string. Used for
/// round-trip storage in the PG `tls_rpt_events.policy_type`
/// column so the DB rows match the report's wire format exactly.
pub(super) fn policy_type_str(p: PolicyType) -> &'static str {
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
pub(super) fn canonical_policy_str_to_type(s: &str) -> PolicyType {
    match s {
        "sts" => PolicyType::Sts,
        "tlsa" => PolicyType::Tlsa,
        _ => PolicyType::NoPolicyFound,
    }
}

pub(super) fn failure_type_str(f: FailureType) -> &'static str {
    f.as_str()
}

pub(super) fn str_to_failure_type(s: &str) -> FailureType {
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
pub(super) fn truncate(s: &str, n: usize) -> String {
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
}
