//! Error type for MTA-STS parsers and decisions.

/// Errors returned by `mailrs-mta-sts` parsers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtaStsError {
    /// TXT record didn't start with `v=STSv1`.
    NotAnStsRecord,
    /// TXT record missing the required `id=` tag.
    MissingId,
    /// Policy file missing a required field (`version`, `mode`, `mx`, or `max_age`).
    MissingField(&'static str),
    /// `version` field was not `STSv1`.
    UnsupportedVersion(String),
    /// `mode` value was not one of `enforce|testing|none`.
    InvalidMode(String),
    /// `max_age` not a non-negative integer.
    InvalidMaxAge(String),
    /// `id` value too long (max 32 chars per RFC 8461 §3.1).
    IdTooLong(usize),
}

impl std::fmt::Display for MtaStsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnStsRecord => write!(f, "not an MTA-STS record (missing v=STSv1)"),
            Self::MissingId => write!(f, "MTA-STS record missing required id= tag"),
            Self::MissingField(n) => write!(f, "MTA-STS policy missing required field: {n}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported MTA-STS version: {v}"),
            Self::InvalidMode(m) => write!(f, "invalid mode: {m}"),
            Self::InvalidMaxAge(s) => write!(f, "invalid max_age: {s}"),
            Self::IdTooLong(n) => write!(f, "id too long: {n} chars (max 32)"),
        }
    }
}

impl std::error::Error for MtaStsError {}
