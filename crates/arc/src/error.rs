//! Error type for ARC parsing + verification.

/// Errors returned by ARC parsers and the chain verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArcError {
    /// Header value missing a required tag.
    MissingTag(String),
    /// Tag value malformed.
    InvalidTag(String),
    /// `cv=` value not one of `none` / `pass` / `fail`.
    InvalidCv(String),
    /// `i=` instance value out of range (must be 1-50 per RFC 8617 §4.2.1).
    InvalidInstance(u32),
    /// `a=` algorithm not supported.
    UnsupportedAlgorithm(String),
    /// Sets in a chain are not contiguous from `i=1`.
    NonContiguousChain {
        /// First missing instance.
        missing: u32,
    },
    /// Chain has more than 50 sets (RFC 8617 §4.2.1 limit).
    ChainTooLong(usize),
    /// One of AAR / AMS / AS is missing for an instance number that
    /// appeared on another header.
    IncompleteSet {
        /// Instance whose triplet is incomplete.
        instance: u32,
        /// Which header type is missing (`"aar"` / `"ams"` / `"seal"`).
        missing: &'static str,
    },
    /// DNS lookup for the public key TXT record failed.
    Dns(String),
    /// Public-key TXT record present but unparseable.
    InvalidPublicKey(String),
    /// Cryptographic verification of an AMS or AS failed.
    SignatureMismatch {
        /// Which header type's signature failed.
        header: &'static str,
        /// Instance number whose signature failed.
        instance: u32,
    },
    /// AMS body hash (`bh=`) did not match the recomputed hash of the
    /// canonicalized body.
    BodyHashMismatch,
    /// A base64 tag (`b=` / `bh=`) failed to decode.
    InvalidBase64(String),
    /// The raw message has no detectable end-of-headers terminator
    /// (no CRLF CRLF, no LF LF).
    MalformedMessage,
}

impl std::fmt::Display for ArcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTag(t) => write!(f, "missing required tag: {t}"),
            Self::InvalidTag(t) => write!(f, "invalid tag: {t}"),
            Self::InvalidCv(v) => write!(f, "invalid cv= value: {v}"),
            Self::InvalidInstance(i) => {
                write!(f, "invalid i= value: {i} (must be 1..=50)")
            }
            Self::UnsupportedAlgorithm(a) => write!(f, "unsupported algorithm: {a}"),
            Self::NonContiguousChain { missing } => {
                write!(f, "chain not contiguous from i=1; missing i={missing}")
            }
            Self::ChainTooLong(n) => {
                write!(f, "chain too long: {n} sets (max 50 per RFC 8617 §4.2.1)")
            }
            Self::IncompleteSet { instance, missing } => {
                write!(f, "incomplete ARC set i={instance}: missing {missing}")
            }
            Self::Dns(msg) => write!(f, "DNS lookup failed: {msg}"),
            Self::InvalidPublicKey(msg) => write!(f, "invalid public key: {msg}"),
            Self::SignatureMismatch { header, instance } => {
                write!(f, "signature mismatch on {header} i={instance}")
            }
            Self::BodyHashMismatch => write!(f, "body hash (bh=) mismatch"),
            Self::InvalidBase64(tag) => write!(f, "invalid base64 in tag: {tag}"),
            Self::MalformedMessage => write!(f, "malformed message: no end-of-headers"),
        }
    }
}

impl std::error::Error for ArcError {}
