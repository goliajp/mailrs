//! Error + result types per RFC 6376 §3.9 / RFC 8601 §2.7.1.

use std::fmt;

/// Verification outcome (RFC 8601 §2.7.1 — same vocabulary as
/// `Authentication-Results: dkim=...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DkimResult {
    /// No DKIM-Signature header present.
    None,
    /// Signature verified successfully.
    Pass,
    /// Signature was present but verification failed (wrong hash,
    /// expired, body modified, etc.).
    Fail,
    /// Policy explicitly rejects the message; rarely produced by
    /// raw verification but reserved.
    Policy,
    /// Signature is malformed — `b=` not base64, tag missing, etc.
    /// Per RFC 8601 this is distinct from Fail.
    Neutral,
    /// Temporary error during verification (DNS lookup timeout).
    TempError,
    /// Permanent error (DNS NXDOMAIN for selector, key revoked,
    /// unsupported algorithm).
    PermError,
}

impl DkimResult {
    /// RFC 8601 wire-form string (lowercase).
    pub fn as_str(&self) -> &'static str {
        match self {
            DkimResult::None => "none",
            DkimResult::Pass => "pass",
            DkimResult::Fail => "fail",
            DkimResult::Policy => "policy",
            DkimResult::Neutral => "neutral",
            DkimResult::TempError => "temperror",
            DkimResult::PermError => "permerror",
        }
    }
}

impl fmt::Display for DkimResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Internal error category from the parser + verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DkimError {
    /// DKIM-Signature header missing or malformed.
    MissingHeader,
    /// Required tag (v, a, b, bh, d, h, s) missing.
    MissingTag(String),
    /// Tag value couldn't be parsed.
    InvalidTag(String),
    /// Base64 decode failed for b= or bh=.
    InvalidBase64(String),
    /// DNS lookup for selector failed.
    DnsTempError(String),
    /// DNS returned NXDOMAIN or unparseable TXT for selector.
    DnsPermError(String),
    /// Public key TXT record malformed.
    InvalidKey(String),
    /// Unsupported algorithm (we support rsa-sha256 in 1.0).
    UnsupportedAlgorithm(String),
    /// Unsupported canonicalization combination.
    UnsupportedCanon(String),
    /// Body hash (bh=) mismatch — body was modified in transit.
    BodyHashMismatch,
    /// Header signature failed RSA verify — body unchanged but
    /// headers / signature don't match.
    SignatureMismatch,
    /// Signature expired (x= tag past).
    Expired,
}

impl fmt::Display for DkimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DkimError::MissingHeader => f.write_str("no DKIM-Signature header"),
            DkimError::MissingTag(t) => write!(f, "missing required tag {t}="),
            DkimError::InvalidTag(t) => write!(f, "invalid tag: {t}"),
            DkimError::InvalidBase64(t) => write!(f, "invalid base64 in tag {t}"),
            DkimError::DnsTempError(s) => write!(f, "DNS temporary error: {s}"),
            DkimError::DnsPermError(s) => write!(f, "DNS permanent error: {s}"),
            DkimError::InvalidKey(s) => write!(f, "invalid public key: {s}"),
            DkimError::UnsupportedAlgorithm(a) => write!(f, "unsupported algorithm: {a}"),
            DkimError::UnsupportedCanon(c) => write!(f, "unsupported canonicalization: {c}"),
            DkimError::BodyHashMismatch => f.write_str("body hash mismatch"),
            DkimError::SignatureMismatch => f.write_str("signature mismatch"),
            DkimError::Expired => f.write_str("signature expired"),
        }
    }
}

impl std::error::Error for DkimError {}

impl DkimError {
    /// Map an error to the public [`DkimResult`].
    pub fn to_result(&self) -> DkimResult {
        match self {
            DkimError::MissingHeader => DkimResult::None,
            DkimError::DnsTempError(_) => DkimResult::TempError,
            DkimError::DnsPermError(_)
            | DkimError::InvalidKey(_)
            | DkimError::UnsupportedAlgorithm(_)
            | DkimError::UnsupportedCanon(_)
            | DkimError::Expired => DkimResult::PermError,
            DkimError::MissingTag(_) | DkimError::InvalidTag(_) | DkimError::InvalidBase64(_) => {
                DkimResult::Neutral
            }
            DkimError::BodyHashMismatch | DkimError::SignatureMismatch => DkimResult::Fail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_as_str_matches_rfc_8601() {
        assert_eq!(DkimResult::None.as_str(), "none");
        assert_eq!(DkimResult::Pass.as_str(), "pass");
        assert_eq!(DkimResult::Fail.as_str(), "fail");
        assert_eq!(DkimResult::Neutral.as_str(), "neutral");
        assert_eq!(DkimResult::TempError.as_str(), "temperror");
        assert_eq!(DkimResult::PermError.as_str(), "permerror");
    }

    #[test]
    fn error_to_result_classification() {
        assert_eq!(DkimError::MissingHeader.to_result(), DkimResult::None);
        assert_eq!(
            DkimError::DnsTempError("x".into()).to_result(),
            DkimResult::TempError
        );
        assert_eq!(
            DkimError::DnsPermError("x".into()).to_result(),
            DkimResult::PermError
        );
        assert_eq!(
            DkimError::MissingTag("a".into()).to_result(),
            DkimResult::Neutral
        );
        assert_eq!(DkimError::BodyHashMismatch.to_result(), DkimResult::Fail);
        assert_eq!(DkimError::SignatureMismatch.to_result(), DkimResult::Fail);
        assert_eq!(DkimError::Expired.to_result(), DkimResult::PermError);
    }

    #[test]
    fn display_contains_context() {
        let e = DkimError::MissingTag("a".into());
        let s = format!("{e}");
        assert!(s.contains('a'));
    }
}
