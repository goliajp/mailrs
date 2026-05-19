use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// compute HMAC-SHA256 signature of payload using the given secret
/// returns lowercase hex-encoded signature
pub fn sign_payload(secret: &[u8], payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key size");
    mac.update(payload);
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// verify a hex-encoded HMAC-SHA256 signature using timing-safe comparison
#[allow(dead_code)]
pub fn verify_signature(secret: &[u8], payload: &[u8], signature: &str) -> bool {
    let Ok(sig_bytes) = hex::decode(signature) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key size");
    mac.update(payload);
    mac.verify_slice(&sig_bytes).is_ok()
}

/// format signature for the X-Mailrs-Signature header
pub fn format_signature_header(signature: &str) -> String {
    format!("sha256={signature}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_payload_is_deterministic() {
        let sig1 = sign_payload(b"secret", b"payload");
        let sig2 = sign_payload(b"secret", b"payload");
        assert_eq!(sig1, sig2);
        // should be 64 hex chars (32 bytes)
        assert_eq!(sig1.len(), 64);
        assert!(sig1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn verify_signature_succeeds_with_correct_sig() {
        let sig = sign_payload(b"my-secret", b"hello world");
        assert!(verify_signature(b"my-secret", b"hello world", &sig));
    }

    #[test]
    fn verify_signature_fails_with_wrong_secret() {
        let sig = sign_payload(b"correct-secret", b"hello world");
        assert!(!verify_signature(b"wrong-secret", b"hello world", &sig));
    }

    #[test]
    fn verify_signature_fails_with_tampered_payload() {
        let sig = sign_payload(b"secret", b"original payload");
        assert!(!verify_signature(b"secret", b"tampered payload", &sig));
    }

    #[test]
    fn verify_signature_fails_with_invalid_hex() {
        assert!(!verify_signature(b"secret", b"payload", "not-valid-hex-zzzz"));
    }

    #[test]
    fn format_signature_header_adds_prefix() {
        let sig = sign_payload(b"secret", b"payload");
        let header = format_signature_header(&sig);
        assert!(header.starts_with("sha256="));
        assert_eq!(header, format!("sha256={sig}"));
    }
}
