//! Failure-type enum for TLSRPT report `failure-details` (RFC 8460 §4.3).
//!
//! Every reported TLS failure carries a `result-type` from one of 14
//! controlled vocabulary values. They split into two classes:
//!
//! - **Negotiation-stage failures** — STARTTLS not offered, cert
//!   mismatch, expired cert, untrusted CA, generic validation error.
//! - **Policy-stage failures** — couldn't fetch STS / DANE policy,
//!   policy was malformed, no policy where one was required, MX
//!   didn't match policy.
//!
//! The enum is serde-serialized as the RFC's canonical kebab-case
//! string. Round-trippable to/from the JSON report.

use serde::{Deserialize, Serialize};

/// `result-type` value for one failure entry. Every variant maps
/// 1:1 to a string in RFC 8460 §4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureType {
    // ---------- Negotiation-stage ----------
    /// Receiving MX did not advertise STARTTLS (and policy required it).
    StarttlsNotSupported,
    /// Certificate's common name / SAN did not match the receiving MX.
    CertificateHostMismatch,
    /// Certificate is expired or not yet valid.
    CertificateExpired,
    /// Certificate not trusted (CA path failed).
    CertificateNotTrusted,
    /// Generic TLS validation failure not covered by the more specific
    /// types above.
    ValidationFailure,

    // ---------- STS policy-stage ----------
    /// Couldn't fetch the STS policy from `mta-sts.<domain>`.
    StsPolicyFetchError,
    /// STS policy fetched but failed to parse.
    StsPolicyInvalid,
    /// HTTPS WebPKI validation of the STS policy host failed.
    StsWebpkiInvalid,

    // ---------- TLSA / DANE policy-stage ----------
    /// TLSA record present but malformed.
    TlsaInvalid,
    /// DNSSEC validation failed for a record that required it.
    DnssecInvalid,
    /// DANE was required but no TLSA records were published.
    DaneRequired,
    /// Receiving environment doesn't support DNSSEC (cannot anchor DANE).
    DnssecNotSupported,

    // ---------- Generic policy-stage ----------
    /// The MX hostname did not match the published policy's MX pattern.
    MxMismatch,
    /// No policy published for the receiving domain (sender's policy
    /// expected one). Often the result of a TLSRPT-only domain.
    PolicyNotPublished,
}

impl FailureType {
    /// Return the canonical RFC 8460 §4.3 string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StarttlsNotSupported => "starttls-not-supported",
            Self::CertificateHostMismatch => "certificate-host-mismatch",
            Self::CertificateExpired => "certificate-expired",
            Self::CertificateNotTrusted => "certificate-not-trusted",
            Self::ValidationFailure => "validation-failure",
            Self::StsPolicyFetchError => "sts-policy-fetch-error",
            Self::StsPolicyInvalid => "sts-policy-invalid",
            Self::StsWebpkiInvalid => "sts-webpki-invalid",
            Self::TlsaInvalid => "tlsa-invalid",
            Self::DnssecInvalid => "dnssec-invalid",
            Self::DaneRequired => "dane-required",
            Self::DnssecNotSupported => "dnssec-not-supported",
            Self::MxMismatch => "mx-mismatch",
            Self::PolicyNotPublished => "policy-not-published",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_round_trips_through_serde() {
        let variants = [
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
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            // serde emits with double quotes.
            assert_eq!(json, format!("\"{}\"", v.as_str()));
            let back: FailureType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, v);
        }
    }

    #[test]
    fn all_variants_round_trip_to_kebab_case() {
        // Spot-check a known RFC §4.3 spelling.
        assert_eq!(
            FailureType::CertificateHostMismatch.as_str(),
            "certificate-host-mismatch"
        );
        let json = serde_json::to_string(&FailureType::StsPolicyFetchError).unwrap();
        assert_eq!(json, "\"sts-policy-fetch-error\"");
    }
}
