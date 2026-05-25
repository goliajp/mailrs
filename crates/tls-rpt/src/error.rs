//! Error type for TLSRPT parsing.

/// Errors returned by [`crate::record::TlsRptRecord::parse`] and the
/// report-building helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsRptError {
    /// The TXT record does not begin with `v=TLSRPTv1`.
    NotATlsRptRecord,
    /// `rua=` tag missing (RFC 8460 §3 requires at least one).
    MissingRua,
    /// `v=` value not `TLSRPTv1`.
    UnsupportedVersion(String),
    /// A required field on a [`crate::report::Report`] was empty / unset
    /// before [`crate::report::ReportBuilder::build`] was called.
    MissingField(&'static str),
    /// Invalid endpoint URL (rua=) — must be `mailto:` or `https:`.
    InvalidEndpoint(String),
}

impl std::fmt::Display for TlsRptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotATlsRptRecord => {
                write!(f, "not a TLSRPT record (missing v=TLSRPTv1)")
            }
            Self::MissingRua => write!(f, "TLSRPT record missing required rua= tag"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported TLSRPT version: {v} (expected TLSRPTv1)")
            }
            Self::MissingField(name) => write!(f, "report missing required field: {name}"),
            Self::InvalidEndpoint(url) => {
                write!(f, "invalid rua endpoint: {url} (must be mailto: or https:)")
            }
        }
    }
}

impl std::error::Error for TlsRptError {}
