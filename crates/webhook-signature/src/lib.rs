#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 sign a payload. Returns a lowercase 64-char hex string
/// (32 bytes of HMAC output).
///
/// Use this on the **sender** side before delivering a webhook:
///
/// ```
/// use mailrs_webhook_signature::{sign, format_header};
/// let secret = b"shared-32-byte-secret";
/// let payload = br#"{"event":"new_message"}"#;
/// let sig = sign(secret, payload);
/// let header = format_header(&sig);
/// // → "sha256=ab12cd..."
/// // POST with header X-Webhook-Signature: <header>
/// ```
pub fn sign(secret: &[u8], payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a hex-encoded HMAC-SHA256 signature against a payload.
/// Returns `true` if the signature matches under `secret`, `false`
/// otherwise.
///
/// The HMAC byte comparison is **constant-time** via
/// `hmac::Mac::verify_slice`, so an attacker probing this function
/// cannot recover the secret by timing analysis.
///
/// The `signature` argument is the bare hex string (no `sha256=`
/// prefix). Use [`parse_header`] first if you have the full header
/// value.
///
/// ```
/// use mailrs_webhook_signature::{sign, verify};
/// let secret = b"k";
/// let payload = b"hello";
/// let sig = sign(secret, payload);
/// assert!(verify(secret, payload, &sig));
/// assert!(!verify(b"wrong", payload, &sig));
/// ```
pub fn verify(secret: &[u8], payload: &[u8], signature: &str) -> bool {
    let Ok(sig_bytes) = hex::decode(signature) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload);
    mac.verify_slice(&sig_bytes).is_ok()
}

/// Verify a header-format signature value (`sha256=<hash>` or bare
/// hex) against a payload. Convenience wrapper that strips the
/// `sha256=` prefix if present.
///
/// Returns `false` for any malformed input (no panic).
///
/// ```
/// use mailrs_webhook_signature::{sign, format_header, verify_header};
/// let secret = b"k";
/// let payload = b"hello";
/// let header = format_header(&sign(secret, payload));
/// assert!(verify_header(secret, payload, &header));
/// // Bare hex (no prefix) also accepted
/// let bare = sign(secret, payload);
/// assert!(verify_header(secret, payload, &bare));
/// ```
pub fn verify_header(secret: &[u8], payload: &[u8], header_value: &str) -> bool {
    let sig = parse_header(header_value);
    verify(secret, payload, sig)
}

/// Verify a payload against **multiple** secrets (current + previous,
/// for rotation). Returns `true` if ANY secret in `secrets` matches.
///
/// Use this during a secret-rotation window: the receiver verifies
/// with the new secret first, falls back to the old one. After the
/// rotation window closes, drop the old secret.
///
/// ```
/// use mailrs_webhook_signature::{sign, verify_any};
/// // Sender used the OLD secret to sign
/// let sig = sign(b"old-secret", b"payload");
/// // Receiver knows both current + previous secret during rotation
/// assert!(verify_any(&[b"new-secret", b"old-secret"], b"payload", &sig));
/// // Neither matches → false
/// assert!(!verify_any(&[b"x", b"y"], b"payload", &sig));
/// ```
pub fn verify_any(secrets: &[&[u8]], payload: &[u8], signature: &str) -> bool {
    let Ok(sig_bytes) = hex::decode(signature) else {
        return false;
    };
    for secret in secrets {
        let mut mac =
            HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
        mac.update(payload);
        if mac.verify_slice(&sig_bytes).is_ok() {
            return true;
        }
    }
    false
}

/// Format a hex signature into the canonical header value
/// `sha256=<hash>`. Use this when constructing the request header on
/// the sender side.
pub fn format_header(signature: &str) -> String {
    let mut out = String::with_capacity(7 + signature.len());
    out.push_str("sha256=");
    out.push_str(signature);
    out
}

/// Parse a header value into the bare hex signature. Accepts the
/// canonical `sha256=<hash>` form OR bare hex. Whitespace is trimmed.
///
/// Returns the bare hex string as a borrowed `&str`. If the input
/// has no `sha256=` prefix, the trimmed input is returned as-is —
/// callers can pass either form interchangeably to [`verify`].
///
/// ```
/// use mailrs_webhook_signature::parse_header;
/// assert_eq!(parse_header("sha256=abc123"), "abc123");
/// assert_eq!(parse_header("abc123"), "abc123");
/// assert_eq!(parse_header("  sha256=abc123  "), "abc123");
/// ```
pub fn parse_header(header_value: &str) -> &str {
    let trimmed = header_value.trim();
    trimmed.strip_prefix("sha256=").unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_is_deterministic() {
        let s1 = sign(b"secret", b"payload");
        let s2 = sign(b"secret", b"payload");
        assert_eq!(s1, s2);
    }

    #[test]
    fn sign_output_is_64_hex_chars() {
        let s = sign(b"k", b"p");
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_with_correct_secret_returns_true() {
        let sig = sign(b"my-secret", b"hello world");
        assert!(verify(b"my-secret", b"hello world", &sig));
    }

    #[test]
    fn verify_with_wrong_secret_returns_false() {
        let sig = sign(b"correct", b"hello");
        assert!(!verify(b"wrong", b"hello", &sig));
    }

    #[test]
    fn verify_with_tampered_payload_returns_false() {
        let sig = sign(b"secret", b"original");
        assert!(!verify(b"secret", b"tampered", &sig));
    }

    #[test]
    fn verify_with_invalid_hex_returns_false_no_panic() {
        assert!(!verify(b"secret", b"payload", "not-valid-hex-zzzz"));
        assert!(!verify(b"secret", b"payload", ""));
        assert!(!verify(b"secret", b"payload", "0"));  // odd length
    }

    #[test]
    fn format_header_adds_sha256_prefix() {
        assert_eq!(format_header("abc123"), "sha256=abc123");
    }

    #[test]
    fn format_header_empty_signature() {
        assert_eq!(format_header(""), "sha256=");
    }

    #[test]
    fn parse_header_strips_prefix() {
        assert_eq!(parse_header("sha256=abc123"), "abc123");
    }

    #[test]
    fn parse_header_accepts_bare_hex() {
        assert_eq!(parse_header("abc123"), "abc123");
    }

    #[test]
    fn parse_header_trims_whitespace() {
        assert_eq!(parse_header("  sha256=abc123  "), "abc123");
        assert_eq!(parse_header("\tabc123\n"), "abc123");
    }

    #[test]
    fn parse_header_other_algo_prefix_passes_through() {
        // sha512= isn't stripped; we only know about sha256.
        assert_eq!(parse_header("sha512=abc"), "sha512=abc");
    }

    #[test]
    fn verify_header_with_prefix() {
        let secret = b"k";
        let payload = b"p";
        let header = format_header(&sign(secret, payload));
        assert!(verify_header(secret, payload, &header));
    }

    #[test]
    fn verify_header_with_bare_hex() {
        let secret = b"k";
        let payload = b"p";
        let bare = sign(secret, payload);
        assert!(verify_header(secret, payload, &bare));
    }

    #[test]
    fn verify_header_with_extra_whitespace() {
        let secret = b"k";
        let payload = b"p";
        let header = format!("  sha256={}  ", sign(secret, payload));
        assert!(verify_header(secret, payload, &header));
    }

    #[test]
    fn verify_any_matches_first_secret() {
        let sig = sign(b"first", b"payload");
        assert!(verify_any(&[b"first", b"second"], b"payload", &sig));
    }

    #[test]
    fn verify_any_matches_second_secret() {
        let sig = sign(b"second", b"payload");
        assert!(verify_any(&[b"first", b"second"], b"payload", &sig));
    }

    #[test]
    fn verify_any_returns_false_when_no_secret_matches() {
        let sig = sign(b"unrelated", b"payload");
        assert!(!verify_any(&[b"a", b"b", b"c"], b"payload", &sig));
    }

    #[test]
    fn verify_any_empty_secrets_list_returns_false() {
        let sig = sign(b"k", b"p");
        assert!(!verify_any(&[], b"p", &sig));
    }

    #[test]
    fn empty_payload_signs_and_verifies() {
        let sig = sign(b"k", b"");
        assert!(verify(b"k", b"", &sig));
        assert!(!verify(b"k", b"x", &sig));
    }

    #[test]
    fn empty_secret_is_accepted_by_hmac() {
        // HMAC accepts any key length, including empty. This is technically
        // insecure (no real key material) but the API doesn't reject it —
        // documented contract.
        let sig = sign(b"", b"payload");
        assert!(verify(b"", b"payload", &sig));
    }

    #[test]
    fn long_payload_works() {
        // 100 KB payload — verify no overflow / panic
        let payload: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
        let sig = sign(b"k", &payload);
        assert!(verify(b"k", &payload, &sig));
    }

    #[test]
    fn long_secret_distinguishes() {
        let s1: Vec<u8> = b"a".repeat(1000).to_vec();
        let s2: Vec<u8> = b"b".repeat(1000).to_vec();
        let sig = sign(&s1, b"p");
        assert!(verify(&s1, b"p", &sig));
        assert!(!verify(&s2, b"p", &sig));
    }

    #[test]
    fn signature_changes_with_payload_change() {
        let s1 = sign(b"k", b"a");
        let s2 = sign(b"k", b"b");
        assert_ne!(s1, s2);
    }

    #[test]
    fn signature_lowercase_hex_only() {
        let s = sign(b"k", b"p");
        assert!(s.chars().all(|c| !c.is_ascii_uppercase()));
    }
}
