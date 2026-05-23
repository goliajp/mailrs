//! Standalone signature verification primitives — extracted from
//! [`crate::verifier`] so other crates in the email-auth family
//! (notably `mailrs-arc`'s AMS / AS verify) can reuse the same
//! RSA-SHA256 + Ed25519-SHA256 verifier without depending on the
//! DKIM-Signature header layout.
//!
//! Both DKIM (RFC 6376) and ARC (RFC 8617) sign a SHA-256 hash of a
//! canonicalized header block with the same algorithm set. Once you've
//! produced the canonicalized hash-input bytes, the actual signature
//! check is identical — that's what [`verify_signature`] is for.

use base64::Engine as _;
use rsa::Pkcs1v15Sign;
use rsa::pkcs8::DecodePublicKey;
use sha2::{Digest, Sha256};

use crate::error::DkimError;
use crate::header::Algorithm;

/// Parse a public-key DNS TXT record into the raw key bytes referenced
/// by the `p=` tag.
///
/// Format: `v=DKIM1; k=rsa; p=<base64-DER>` (RFC 6376 §3.6.1). The
/// returned bytes are the base64-decoded `p=` payload:
///
/// - **RsaSha256**: PKCS8-DER-encoded `SubjectPublicKeyInfo`.
/// - **Ed25519Sha256** (RFC 8463 §3): raw 32-byte little-endian
///   public key — NOT PKCS8.
///
/// Whitespace inside `p=` is stripped (DKIM TXT records are commonly
/// split across multiple TXT segments / lines).
///
/// Returns [`DkimError::InvalidKey`] if `p=` is missing, empty (key
/// revocation marker per spec), or not valid base64.
pub fn extract_public_key(txt: &str) -> Result<Vec<u8>, DkimError> {
    let p_value = txt
        .split(';')
        .find_map(|t| {
            let t = t.trim();
            t.strip_prefix("p=")
        })
        .ok_or_else(|| DkimError::InvalidKey("p= tag missing".into()))?;
    let p_value = p_value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
        .collect::<String>();
    if p_value.is_empty() {
        return Err(DkimError::InvalidKey("p= empty (key revoked)".into()));
    }
    base64::engine::general_purpose::STANDARD
        .decode(p_value.as_bytes())
        .map_err(|e| DkimError::InvalidKey(format!("p= base64 decode: {e}")))
}

/// Verify an RSA-SHA256 or Ed25519-SHA256 signature over `signed_data`
/// against the supplied public key.
///
/// `key_bytes` is the same byte string [`extract_public_key`] returns —
/// PKCS8 DER for RSA, raw 32-byte key for Ed25519.
///
/// `sig_bytes` is the raw signature (base64-decoded from the `b=` tag).
/// For Ed25519 it must be exactly 64 bytes.
///
/// Returns `Ok(())` on success or [`DkimError::SignatureMismatch`] on
/// any failure (wrong signature, malformed key, length mismatch).
pub fn verify_signature(
    algorithm: Algorithm,
    key_bytes: &[u8],
    signed_data: &[u8],
    sig_bytes: &[u8],
) -> Result<(), DkimError> {
    match algorithm {
        Algorithm::RsaSha256 => {
            let public_key = rsa::RsaPublicKey::from_public_key_der(key_bytes)
                .map_err(|e| DkimError::InvalidKey(format!("RSA PKCS8 decode failed: {e}")))?;
            let mut hasher = Sha256::new();
            hasher.update(signed_data);
            let digest = hasher.finalize();
            let scheme = Pkcs1v15Sign::new::<Sha256>();
            public_key
                .verify(scheme, &digest, sig_bytes)
                .map_err(|_| DkimError::SignatureMismatch)
        }
        Algorithm::Ed25519Sha256 => {
            if key_bytes.len() != 32 {
                return Err(DkimError::InvalidKey(format!(
                    "ed25519 key wrong length: {} (expected 32)",
                    key_bytes.len()
                )));
            }
            let mut key_arr = [0u8; 32];
            key_arr.copy_from_slice(key_bytes);
            let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_arr)
                .map_err(|e| DkimError::InvalidKey(format!("ed25519 key decode: {e}")))?;
            let mut hasher = Sha256::new();
            hasher.update(signed_data);
            let digest = hasher.finalize();
            if sig_bytes.len() != 64 {
                return Err(DkimError::SignatureMismatch);
            }
            let mut sig_arr = [0u8; 64];
            sig_arr.copy_from_slice(sig_bytes);
            let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
            use ed25519_dalek::Verifier as _;
            verifying_key
                .verify(&digest, &signature)
                .map_err(|_| DkimError::SignatureMismatch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pubkey_finds_p_tag() {
        let txt = "v=DKIM1; k=rsa; p=YWJjZA==";
        assert_eq!(extract_public_key(txt).unwrap(), b"abcd");
    }

    #[test]
    fn extract_pubkey_rejects_missing_p() {
        let txt = "v=DKIM1; k=rsa";
        assert!(matches!(
            extract_public_key(txt).unwrap_err(),
            DkimError::InvalidKey(_)
        ));
    }

    #[test]
    fn extract_pubkey_rejects_empty_p() {
        let txt = "v=DKIM1; k=rsa; p=";
        let err = extract_public_key(txt).unwrap_err();
        assert!(matches!(err, DkimError::InvalidKey(ref m) if m.contains("revoked")));
    }

    #[test]
    fn extract_pubkey_strips_wsp_in_p() {
        let txt = "v=DKIM1; k=rsa; p=YWJj\r\n ZA==";
        assert_eq!(extract_public_key(txt).unwrap(), b"abcd");
    }

    #[test]
    fn verify_signature_rejects_wrong_length_ed25519_sig() {
        let key = [0u8; 32];
        let r = verify_signature(Algorithm::Ed25519Sha256, &key, b"data", &[0u8; 63]);
        assert!(matches!(r, Err(DkimError::SignatureMismatch)));
    }

    #[test]
    fn verify_signature_rejects_wrong_length_ed25519_key() {
        let key = [0u8; 31];
        let r = verify_signature(Algorithm::Ed25519Sha256, &key, b"data", &[0u8; 64]);
        assert!(matches!(r, Err(DkimError::InvalidKey(_))));
    }
}
