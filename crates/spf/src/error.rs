//! Error + result types per RFC 7208 §2.6.

use std::fmt;

/// SPF verification outcome (RFC 7208 §2.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpfResult {
    /// No SPF record at the domain — `Result.None`.
    None,
    /// Mechanism produced an explicit `+` match.
    Pass,
    /// Hard fail (`-` qualifier).
    Fail,
    /// Soft fail (`~` qualifier) — accept but mark suspicious.
    SoftFail,
    /// Neutral (`?` qualifier) — no policy statement.
    Neutral,
    /// Permanent error: malformed SPF record or per-record limit
    /// (10 DNS lookups, max recursion, etc.) — never going to work,
    /// reject or quarantine.
    PermError,
    /// Temporary error: DNS lookup failure (SERVFAIL, timeout) —
    /// retry later.
    TempError,
}

impl SpfResult {
    /// Lowercase wire form per RFC 7001 §2.7.2 (used in
    /// `Authentication-Results: spf=...` headers).
    pub fn as_str(&self) -> &'static str {
        match self {
            SpfResult::None => "none",
            SpfResult::Pass => "pass",
            SpfResult::Fail => "fail",
            SpfResult::SoftFail => "softfail",
            SpfResult::Neutral => "neutral",
            SpfResult::PermError => "permerror",
            SpfResult::TempError => "temperror",
        }
    }
}

impl fmt::Display for SpfResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Internal error category from the evaluator / parser.
///
/// These are NOT the public verification result — that's [`SpfResult`].
/// `SpfError` is for things that go wrong *inside* the verifier
/// (DNS lookup failed, record malformed, limits hit). Most callers
/// just want [`SpfResult`] and don't need to distinguish further.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpfError {
    /// DNS lookup failed transiently (timeout, SERVFAIL, ...).
    DnsTempError(String),
    /// DNS lookup failed permanently (NXDOMAIN, etc.) — usually
    /// the absence of a TXT record is normal and yields
    /// [`SpfResult::None`], not this.
    DnsPermError(String),
    /// SPF record TXT string couldn't be parsed.
    InvalidRecord(String),
    /// Exceeded 10 DNS lookups per RFC 7208 §4.6.4.
    TooManyLookups,
    /// Recursion in `include:` chains exceeded sane depth.
    TooMuchRecursion,
    /// Multiple `v=spf1` TXT records found for the same domain
    /// (RFC 7208 §4.5). Per spec → permerror.
    MultipleRecords,
}

impl fmt::Display for SpfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpfError::DnsTempError(s) => write!(f, "DNS temporary error: {s}"),
            SpfError::DnsPermError(s) => write!(f, "DNS permanent error: {s}"),
            SpfError::InvalidRecord(s) => write!(f, "invalid SPF record: {s}"),
            SpfError::TooManyLookups => f.write_str("too many DNS lookups (>10)"),
            SpfError::TooMuchRecursion => f.write_str("too much include: recursion"),
            SpfError::MultipleRecords => f.write_str("multiple v=spf1 records at domain"),
        }
    }
}

impl std::error::Error for SpfError {}

impl SpfError {
    /// Map an error to the appropriate public `SpfResult` per
    /// RFC 7208 §2.6.5 / §2.6.6.
    pub fn to_result(&self) -> SpfResult {
        match self {
            SpfError::DnsTempError(_) => SpfResult::TempError,
            SpfError::DnsPermError(_)
            | SpfError::InvalidRecord(_)
            | SpfError::TooManyLookups
            | SpfError::TooMuchRecursion
            | SpfError::MultipleRecords => SpfResult::PermError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spf_result_as_str_matches_rfc_7001() {
        assert_eq!(SpfResult::None.as_str(), "none");
        assert_eq!(SpfResult::Pass.as_str(), "pass");
        assert_eq!(SpfResult::Fail.as_str(), "fail");
        assert_eq!(SpfResult::SoftFail.as_str(), "softfail");
        assert_eq!(SpfResult::Neutral.as_str(), "neutral");
        assert_eq!(SpfResult::PermError.as_str(), "permerror");
        assert_eq!(SpfResult::TempError.as_str(), "temperror");
    }

    #[test]
    fn spf_result_display_matches_as_str() {
        assert_eq!(format!("{}", SpfResult::Pass), "pass");
        assert_eq!(format!("{}", SpfResult::SoftFail), "softfail");
    }

    #[test]
    fn spf_error_to_result_classification() {
        assert_eq!(
            SpfError::DnsTempError("timeout".into()).to_result(),
            SpfResult::TempError
        );
        assert_eq!(
            SpfError::DnsPermError("nxdomain".into()).to_result(),
            SpfResult::PermError
        );
        assert_eq!(
            SpfError::InvalidRecord("bad mechanism".into()).to_result(),
            SpfResult::PermError
        );
        assert_eq!(SpfError::TooManyLookups.to_result(), SpfResult::PermError);
        assert_eq!(SpfError::TooMuchRecursion.to_result(), SpfResult::PermError);
        assert_eq!(SpfError::MultipleRecords.to_result(), SpfResult::PermError);
    }

    #[test]
    fn spf_error_display_includes_context() {
        let e = SpfError::DnsTempError("connection refused".into());
        let s = format!("{e}");
        assert!(s.contains("connection refused"));
    }
}
