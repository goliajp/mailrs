//! The spool envelope — the cross-process seam between the receiver (writes
//! the spool) and the core (consumes it) in the P6 receiver/core split.
//!
//! In the split, the receiver accepts a message and writes it to a spool
//! maildir **without resolving recipients** — it has no spg. The SMTP envelope
//! (MAIL FROM, the RCPT list, the auth flag, the antispam Junk verdict) is not
//! part of the RFC 822 message, so it would be lost if only the body were
//! written. We prepend it as a single base64-framed synthetic header line, so
//! the spool file is **one atomic maildir write** (no sidecar-file consistency
//! window) and the core can recover the full envelope before delivering.
//!
//! The core strips this header before the message lands in the user's maildir,
//! so the delivered body is byte-identical to the monolith path. The header is
//! only honoured at **offset 0**: a sender-supplied copy deeper in the headers
//! is just an ordinary (ignored) header, not a spoofed envelope.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

/// The synthetic header the receiver prepends to every spool file. Must be the
/// very first bytes of the file.
pub const SPOOL_ENVELOPE_HEADER: &str = "X-Mailrs-Spool-Envelope";

/// Current spool envelope schema version.
pub const SPOOL_SCHEMA_VERSION: u32 = 1;

/// The SMTP envelope + receiver-side verdict carried alongside a spooled
/// message so the core can deliver it without re-deriving session state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpoolEnvelope {
    /// SMTP MAIL FROM (reverse-path). Empty string for the null sender (bounces).
    pub reverse_path: String,
    /// SMTP RCPT TO list (forward-paths), verbatim — the core resolves each.
    pub forward_paths: Vec<String>,
    /// whether the session authenticated (submission). Gates relay in the core.
    pub is_authenticated: bool,
    /// connection id (observability / event correlation).
    pub conn_id: u64,
    /// folder the antispam verdict routed to ("INBOX" or "Junk"). Carried so the
    /// core honours the receiver's Junk decision without re-running antispam.
    pub target_folder: String,
    /// unix seconds the receiver accepted the message.
    pub received_at: u64,
    /// schema version for forward-compat.
    pub schema_version: u32,
}

/// Reasons a spool blob fails to decode. Undecodable files are dead-lettered by
/// the core rather than retried forever.
#[derive(Debug, PartialEq, Eq)]
pub enum SpoolDecodeError {
    /// no envelope header at offset 0 (malformed / spoofed / truncated).
    MissingHeader,
    /// the base64 payload didn't decode.
    BadBase64,
    /// the JSON envelope didn't parse.
    BadJson,
}

impl std::fmt::Display for SpoolDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHeader => f.write_str("spool envelope header missing at offset 0"),
            Self::BadBase64 => f.write_str("spool envelope base64 decode failed"),
            Self::BadJson => f.write_str("spool envelope json parse failed"),
        }
    }
}

impl std::error::Error for SpoolDecodeError {}

/// Encode a spool file: the envelope as a base64 header line, then the raw
/// message body unchanged.
pub fn encode_spool_blob(env: &SpoolEnvelope, body: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(env).expect("SpoolEnvelope always serializes");
    let b64 = B64.encode(&json);
    let mut out = Vec::with_capacity(SPOOL_ENVELOPE_HEADER.len() + b64.len() + body.len() + 4);
    out.extend_from_slice(SPOOL_ENVELOPE_HEADER.as_bytes());
    out.extend_from_slice(b": ");
    out.extend_from_slice(b64.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

/// Decode a spool blob: require the envelope header at offset 0, return the
/// envelope and a slice of the original body (header stripped).
pub fn decode_spool_blob(blob: &[u8]) -> Result<(SpoolEnvelope, &[u8]), SpoolDecodeError> {
    let prefix = format!("{SPOOL_ENVELOPE_HEADER}: ");
    if !blob.starts_with(prefix.as_bytes()) {
        return Err(SpoolDecodeError::MissingHeader);
    }
    let after_prefix = &blob[prefix.len()..];
    let nl = after_prefix
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(SpoolDecodeError::MissingHeader)?;
    // base64 sits between the prefix and the line terminator (\r\n or \n)
    let mut b64_end = nl;
    if b64_end > 0 && after_prefix[b64_end - 1] == b'\r' {
        b64_end -= 1;
    }
    let b64 = &after_prefix[..b64_end];
    let json = B64.decode(b64).map_err(|_| SpoolDecodeError::BadBase64)?;
    let env: SpoolEnvelope =
        serde_json::from_slice(&json).map_err(|_| SpoolDecodeError::BadJson)?;
    let body = &after_prefix[nl + 1..];
    Ok((env, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_env() -> SpoolEnvelope {
        SpoolEnvelope {
            reverse_path: "sender@example.com".into(),
            forward_paths: vec!["alice@smk.ai".into(), "bob@smk.ai".into()],
            is_authenticated: false,
            conn_id: 42,
            target_folder: "INBOX".into(),
            received_at: 1_781_500_000,
            schema_version: SPOOL_SCHEMA_VERSION,
        }
    }

    #[test]
    fn round_trip() {
        let env = sample_env();
        let body = b"Subject: hi\r\n\r\nhello world\r\n";
        let blob = encode_spool_blob(&env, body);
        let (decoded, decoded_body) = decode_spool_blob(&blob).unwrap();
        assert_eq!(decoded, env);
        assert_eq!(decoded_body, body);
    }

    #[test]
    fn body_bytes_unchanged_after_strip() {
        // a body that itself contains CRLFs and a colon-bearing header
        let env = sample_env();
        let body = b"Received: from x\r\nSubject: a:b:c\r\n\r\nbody\r\nline2\r\n";
        let blob = encode_spool_blob(&env, body);
        let (_, decoded_body) = decode_spool_blob(&blob).unwrap();
        assert_eq!(decoded_body, body, "stripped body must be byte-identical");
    }

    #[test]
    fn header_absent_from_stripped_body() {
        let env = sample_env();
        let body = b"Subject: hi\r\n\r\nx";
        let blob = encode_spool_blob(&env, body);
        let (_, decoded_body) = decode_spool_blob(&blob).unwrap();
        assert!(
            !decoded_body
                .windows(SPOOL_ENVELOPE_HEADER.len())
                .any(|w| w == SPOOL_ENVELOPE_HEADER.as_bytes()),
            "the spool envelope header must not appear in the delivered body"
        );
    }

    #[test]
    fn null_sender_round_trips() {
        let mut env = sample_env();
        env.reverse_path = String::new();
        env.forward_paths = vec!["postmaster@smk.ai".into()];
        let blob = encode_spool_blob(&env, b"x");
        let (decoded, _) = decode_spool_blob(&blob).unwrap();
        assert_eq!(decoded.reverse_path, "");
    }

    #[test]
    fn missing_header_rejected() {
        let blob = b"Subject: not a spool file\r\n\r\nbody";
        assert_eq!(
            decode_spool_blob(blob).unwrap_err(),
            SpoolDecodeError::MissingHeader
        );
    }

    #[test]
    fn sender_supplied_header_not_at_offset_zero_is_rejected() {
        // a hostile body that puts the header on line 2 must NOT decode as an envelope
        let blob = b"Subject: hi\r\nX-Mailrs-Spool-Envelope: ZmFrZQ==\r\n\r\nbody";
        assert_eq!(
            decode_spool_blob(blob).unwrap_err(),
            SpoolDecodeError::MissingHeader
        );
    }

    #[test]
    fn bad_base64_rejected() {
        let blob = b"X-Mailrs-Spool-Envelope: !!!not base64!!!\r\nbody";
        assert_eq!(
            decode_spool_blob(blob).unwrap_err(),
            SpoolDecodeError::BadBase64
        );
    }

    #[test]
    fn bad_json_rejected() {
        let b64 = B64.encode(b"{not valid json");
        let blob = format!("{SPOOL_ENVELOPE_HEADER}: {b64}\r\nbody");
        assert_eq!(
            decode_spool_blob(blob.as_bytes()).unwrap_err(),
            SpoolDecodeError::BadJson
        );
    }

    #[test]
    fn lone_lf_terminator_accepted() {
        // tolerate \n without \r (defensive)
        let env = sample_env();
        let json = serde_json::to_vec(&env).unwrap();
        let b64 = B64.encode(&json);
        let blob = format!("{SPOOL_ENVELOPE_HEADER}: {b64}\nbody-here");
        let (decoded, body) = decode_spool_blob(blob.as_bytes()).unwrap();
        assert_eq!(decoded, env);
        assert_eq!(body, b"body-here");
    }
}
