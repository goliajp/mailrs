//! Standalone signature primitives — extracted from [`crate::verifier`]
//! so other crates in the email-auth family (notably `mailrs-arc`'s
//! AMS / AS sign + verify) can reuse the same RSA-SHA256 +
//! Ed25519-SHA256 work without depending on the DKIM-Signature header
//! layout.
//!
//! Both DKIM (RFC 6376) and ARC (RFC 8617) sign a SHA-256 hash of a
//! canonicalized header block with the same algorithm set.
//!
//! ## v3 — RSA via aws-lc-rs
//!
//! The 1.x / 2.x line used the pure-Rust `rsa` crate for RSA sign +
//! verify. Measured ~1.5 ms / RSA-2048 sign, vs ~0.5 ms with
//! `aws-lc-rs` (native AWS-LC binding) which `rustls` already pulls
//! into the workspace. Switching primitives at the same layer
//! reclaims the outbound DKIM perf without changing the
//! `sign_signature` / `verify_signature` external contract.

use std::sync::Arc;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{
    RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY, RSA_PKCS1_SHA256, RsaKeyPair, UnparsedPublicKey,
};
use base64::Engine as _;
use rustls_pki_types::pem::PemObject;

use crate::error::DkimError;
use crate::header::Algorithm;

/// RSA private key handle backing [`CryptoSigningKey::Rsa`]. Wraps an
/// `aws_lc_rs::signature::RsaKeyPair`; parse PKCS#8 PEM via
/// [`RsaSigningKey::from_pkcs8_pem`] or DER via
/// [`RsaSigningKey::from_pkcs8_der`].
///
/// `Clone` is cheap (atomic refcount on the inner `Arc<RsaKeyPair>`) —
/// stash the loaded key in your config and clone per sign call rather
/// than re-parsing the PEM each time.
#[derive(Clone)]
pub struct RsaSigningKey {
    inner: Arc<RsaKeyPair>,
    /// Cached output-buffer length for `RsaKeyPair::sign` (equal to the
    /// public modulus byte length — 256 for 2048-bit, 384 for 3072,
    /// 512 for 4096).
    sig_len: usize,
}

impl RsaSigningKey {
    /// Parse a PKCS#8 PEM-encoded RSA private key.
    pub fn from_pkcs8_pem(pem: &str) -> Result<Self, DkimError> {
        let pkcs8 = rustls_pki_types::PrivatePkcs8KeyDer::from_pem_slice(pem.as_bytes())
            .map_err(|e| DkimError::InvalidKey(format!("PKCS#8 PEM parse: {e}")))?;
        Self::from_pkcs8_der(pkcs8.secret_pkcs8_der())
    }

    /// Parse a PKCS#8 DER-encoded RSA private key.
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self, DkimError> {
        let inner = RsaKeyPair::from_pkcs8(der)
            .map_err(|e| DkimError::InvalidKey(format!("RSA PKCS#8 load: {e}")))?;
        let sig_len = inner.public_modulus_len();
        Ok(Self {
            inner: Arc::new(inner),
            sig_len,
        })
    }

    /// Public modulus length in bytes — also the signature length.
    pub fn public_modulus_len(&self) -> usize {
        self.sig_len
    }
}

impl std::fmt::Debug for RsaSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RsaSigningKey")
            .field("sig_len", &self.sig_len)
            .finish()
    }
}

/// Private key for [`sign_signature`]. Mirrors
/// [`crate::sign::DkimSigningKey`] but lives at the lower
/// "raw bytes in → raw signature out" layer so other crates in the
/// email-auth family (notably `mailrs-arc`'s ARC sealing path) can
/// reuse the same RSA-SHA256 / Ed25519-SHA256 sign primitive
/// without depending on the DKIM signed-message layout.
pub enum CryptoSigningKey<'a> {
    /// RSA — produces an RSA-SHA256 signature.
    Rsa(&'a RsaSigningKey),
    /// Ed25519 — produces an Ed25519-SHA256 signature (RFC 8463).
    Ed25519(&'a ed25519_dalek::SigningKey),
}

impl<'a> CryptoSigningKey<'a> {
    /// Return the [`Algorithm`] this key signs with.
    pub fn algorithm(&self) -> Algorithm {
        match self {
            Self::Rsa(_) => Algorithm::RsaSha256,
            Self::Ed25519(_) => Algorithm::Ed25519Sha256,
        }
    }
}

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

/// Try to strip an ASN.1 DER `SubjectPublicKeyInfo` wrapper from an
/// RSA public-key byte string, returning the inner `RSAPublicKey`
/// (a `SEQUENCE { n INTEGER, e INTEGER }`) that aws-lc-rs's
/// `UnparsedPublicKey` expects.
///
/// DKIM TXT records publish RSA public keys as `SubjectPublicKeyInfo`
/// (PKCS#8); aws-lc-rs's RSA verify path wants just the inner
/// `RSAPublicKey`. We strip the outer SPKI wrapper here.
///
/// Returns `None` (caller falls back to the input slice unchanged)
/// when the bytes don't look like an SPKI wrapper.
///
/// Derived from `mail-auth-0.9.0/src/common/crypto/ring_impls.rs` —
/// the same logic mail-auth uses for the same reason. Keep
/// byte-for-byte compatible.
fn try_strip_rsa_prefix(bytes: &[u8]) -> Option<&[u8]> {
    const DER_OBJECT_ID_TAG: u8 = 0x06;
    const DER_BIT_STRING_TAG: u8 = 0x03;
    const DER_SEQUENCE_TAG: u8 = 0x30;

    if *bytes.first()? != DER_SEQUENCE_TAG {
        return None;
    }
    let (_, bytes) = decode_multi_byte_len(&bytes[1..])?;
    if *bytes.first()? != DER_SEQUENCE_TAG {
        return None;
    }
    let (byte_len, bytes) = decode_multi_byte_len(&bytes[1..])?;
    if *bytes.first()? != DER_OBJECT_ID_TAG || byte_len != 13 {
        return None;
    }
    let bytes = bytes.get(13..)?; // skip the rsaEncryption OID
    if *bytes.first()? != DER_BIT_STRING_TAG {
        return None;
    }
    decode_multi_byte_len(&bytes[1..]).and_then(|(_, bytes)| bytes.get(1..)) // skip unused-bits
}

fn decode_multi_byte_len(bytes: &[u8]) -> Option<(usize, &[u8])> {
    if bytes.first()? & 0x80 == 0 {
        return Some((bytes[0] as usize, &bytes[1..]));
    }
    let len_len = (bytes[0] & 0x7f) as usize;
    if bytes.len() < len_len + 1 {
        return None;
    }
    let mut len = 0;
    for i in 0..len_len {
        len = (len << 8) | bytes[1 + i] as usize;
    }
    Some((len, &bytes[len_len + 1..]))
}

/// Verify an RSA-SHA256 or Ed25519-SHA256 signature over `signed_data`
/// against the supplied public key.
///
/// `key_bytes` is the same byte string [`extract_public_key`] returns —
/// PKCS8 DER `SubjectPublicKeyInfo` for RSA (the SPKI wrapper is
/// stripped internally), raw 32-byte key for Ed25519.
///
/// `sig_bytes` is the raw signature (base64-decoded from the `b=` tag).
/// For Ed25519 it must be exactly 64 bytes.
///
/// **RSA**: backed by `aws-lc-rs`'s
/// `RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY` algorithm — the
/// LEGACY_USE_ONLY suffix is misleading: this is the only PKCS#1 v1.5
/// SHA-256 variant that accepts the 1024-bit RSA keys still common
/// for older DKIM selectors. (Modern selectors are 2048-bit but the
/// crate verifies whatever is published.) aws-lc-rs hashes
/// `signed_data` internally; we do not pre-hash.
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
            // SPKI in → RSAPublicKey for aws-lc-rs. Fall back to the
            // raw input if the strip doesn't match (some signers
            // publish bare RSAPublicKey already).
            let key = try_strip_rsa_prefix(key_bytes).unwrap_or(key_bytes);
            let public =
                UnparsedPublicKey::new(&RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY, key);
            public
                .verify(signed_data, sig_bytes)
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
            if sig_bytes.len() != 64 {
                return Err(DkimError::SignatureMismatch);
            }
            let mut sig_arr = [0u8; 64];
            sig_arr.copy_from_slice(sig_bytes);
            let signature = ed25519_dalek::Signature::from_bytes(&sig_arr);
            // RFC 8463 §3: signature is over the SHA-256 hash of the
            // signed data, NOT the data itself.
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(signed_data);
            let digest = hasher.finalize();
            use ed25519_dalek::Verifier as _;
            verifying_key
                .verify(&digest, &signature)
                .map_err(|_| DkimError::SignatureMismatch)
        }
    }
}

/// Sign `signed_data` and return the raw signature bytes (NOT
/// base64-encoded). Caller is responsible for base64 encoding +
/// header tag assembly.
///
/// The hash algorithm is implied by the key — RSA-SHA256 for
/// [`CryptoSigningKey::Rsa`], Ed25519-SHA256 for
/// [`CryptoSigningKey::Ed25519`] (RFC 8463 §3: signs the SHA-256
/// hash of the signed-header block, not the block itself).
///
/// Mirrors [`verify_signature`] exactly so any caller can pair
/// `sign_signature` + `verify_signature` for a self-consistent
/// sign/verify roundtrip.
///
/// **RSA**: backed by `aws-lc-rs`'s `RSA_PKCS1_SHA256` — hashes
/// `signed_data` internally and signs. ~3× faster than the pure-Rust
/// `rsa` crate primitive used pre-3.0.
///
/// # Errors
///
/// [`DkimError::InvalidKey`] when the underlying signing primitive
/// rejects the operation (very rare — only on malformed keys).
pub fn sign_signature(
    key: &CryptoSigningKey<'_>,
    signed_data: &[u8],
) -> Result<Vec<u8>, DkimError> {
    match key {
        CryptoSigningKey::Rsa(rsa_key) => {
            let mut sig = vec![0u8; rsa_key.sig_len];
            let rng = SystemRandom::new();
            rsa_key
                .inner
                .sign(&RSA_PKCS1_SHA256, &rng, signed_data, &mut sig)
                .map_err(|e| DkimError::InvalidKey(format!("RSA sign failed: {e}")))?;
            Ok(sig)
        }
        CryptoSigningKey::Ed25519(signing_key) => {
            // RFC 8463 §3: signature is over the SHA-256 hash of the
            // signed data, NOT the data itself.
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(signed_data);
            let digest = hasher.finalize();
            use ed25519_dalek::Signer as _;
            let sig = signing_key.sign(&digest);
            Ok(sig.to_bytes().to_vec())
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

    #[test]
    fn sign_then_verify_ed25519_roundtrip() {
        let secret = [7u8; 32];
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        let verifying = signing.verifying_key();
        let key = CryptoSigningKey::Ed25519(&signing);
        let sig = sign_signature(&key, b"hello").unwrap();
        verify_signature(
            Algorithm::Ed25519Sha256,
            verifying.as_bytes(),
            b"hello",
            &sig,
        )
        .unwrap();
    }

    #[test]
    fn sign_then_verify_ed25519_rejects_tampered_data() {
        let secret = [7u8; 32];
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        let verifying = signing.verifying_key();
        let key = CryptoSigningKey::Ed25519(&signing);
        let sig = sign_signature(&key, b"hello").unwrap();
        let r = verify_signature(
            Algorithm::Ed25519Sha256,
            verifying.as_bytes(),
            b"tampered",
            &sig,
        );
        assert!(matches!(r, Err(DkimError::SignatureMismatch)));
    }

    #[test]
    fn signing_key_algorithm_helper() {
        let secret = [0u8; 32];
        let signing = ed25519_dalek::SigningKey::from_bytes(&secret);
        let key = CryptoSigningKey::Ed25519(&signing);
        assert_eq!(key.algorithm(), Algorithm::Ed25519Sha256);
    }

    const TEST_RSA_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDNZMkvBc/kAdQl\n\
GFY6ADYW+guQCJU4x6Zulb4/4fMDUHruL/DR722wV+qKmivIP5SS5X7H+U5X6xha\n\
1r70zJpdpEzyVZtctBZzm1BkKq81BVdL3iJCbVmPPqs2pUOjGsInmM7gEfvhz7CB\n\
q+RQ1fb9iGlBA/WmNqLKiVg1nEVDai6DHzEofI+Ta8ij5yGnYHVLJLsqmJotyvHN\n\
2vi/7kIFigjW/4TOQLcaZGm8AYTEBH4opqvb8C460vLjUFBHeoSqm0vkHWzrwNQx\n\
S29LczFc/WIpQkl1rx5iS8E5QI2u4eCHVElAjZp4IJsyPYBGVN72mi37IGfEjkHS\n\
O2TIUEQhAgMBAAECggEABK/ZlWydB1dxV11cTluF4HVZQTKo8RBBQIHDQyLtUDSM\n\
cZX/eVLs3lrLO9lzyVCGG+oHwBl0y7XOKvh+iAiJNzzSEq+YaX+kiYPQTFDbCasz\n\
CESr5HcpVYb5EjioN/ca2ht3EQ7oAAmkvfjFr4CKb9Omjzi/aMkTYurKbALCY9zk\n\
bx8J9VADe1aAAA54WFxIlJvb72Hrfw8iflFqVZNzykRp6tUvJJgSqLOpfM0ut5zb\n\
0ClgCjSZ7HpehjWVm3KBAOcC7p2TL3erpWoG9BuatgYLLRhW/AzLzXZ3/hSu9kEn\n\
ihws+VXkHxeaIafrck0HQyWnHb9QEcSgfVIhAYztlwKBgQD3O684316go2e6Qf4I\n\
7rF4JwmQiI+NMAQq55AwquZkfuw0N2F9AgyuzGskYvI9Ok+l/wP1e8Mb6JRuP6Nj\n\
dPYTQwzfmyZgdOovxGkZOGE60EQuX/1IS/NbLKQySAphgBVR2FlHnu+VMvha61tm\n\
/5K1ROAB3Ng3FbR7rHJXFjWU+wKBgQDUrUtS3Yj0yHnxA/AL04lsxNrLlinEVDM1\n\
6wPjC2VEXhj2j4JNrVqXG4GVYYEGhkUTjwcTOiZfmHaqMzEFo1aTOoiLrMMLQjmm\n\
jPNkLHsDXcbG5FA0BbzQmlj+ixKPToh2gHfeMfH96YmdROfmvY/TN9yI1FgkLErL\n\
YKatCKWokwKBgC6z25nGuD1oIMQSi0ZssKGd3jSrV1K4a1EfhSFsZzE8uKn0fDn9\n\
FSBABU1OU6w1Q657yeephWXUPZXF97tl8MYauGfVCx7Vdxem5qOY/uT5SqfoAhSS\n\
JFpoyGunKC7a3ywizlq1L1Tj1/50z0NZrAEKDbbMXRuqwflKzh6dV2nZAoGBAImh\n\
N6yBdr7J+bfRz4cntrgv0FONcqv9vUI4O0SzvC35Ivh0OGPiOkytXTd5aND7FTqq\n\
BW8Y43pbpPdRt3ipkj4m0/RnsbTYf4xbjKqX6mdsSVWurIRt7hmkuNDI2RLqRH9D\n\
dc7RzYN+nTKsQ9Jbe/a5ILtfh0apbyGcA2DYxrOHAoGAYYm/jwilVVaH1xSlP52w\n\
BcpT8g8Wqgo4wFOTcyGJScBeFnQO1dhap+KNxCOyM/b2a8p2kQxHPmhIt+iyUpsM\n\
Wob7+tvQ4QgOJAUWByTxMHczAY8Vrl45gxYS29ahbuvjtjPVLgHcaFnZPfun8i6u\n\
/qw9cba4IgRYuEuLJ9bzbAY=\n\
-----END PRIVATE KEY-----";

    #[test]
    fn rsa_signing_key_loads_pem() {
        let k = RsaSigningKey::from_pkcs8_pem(TEST_RSA_PEM).unwrap();
        assert_eq!(k.public_modulus_len(), 256); // 2048-bit
    }

    #[test]
    fn rsa_signing_key_rejects_garbage_pem() {
        let r = RsaSigningKey::from_pkcs8_pem("garbage");
        assert!(matches!(r, Err(DkimError::InvalidKey(_))));
    }

    #[test]
    fn sign_rsa_produces_modulus_len_signature() {
        let key = RsaSigningKey::from_pkcs8_pem(TEST_RSA_PEM).unwrap();
        let cs = CryptoSigningKey::Rsa(&key);
        let sig = sign_signature(&cs, b"hello world").unwrap();
        assert_eq!(sig.len(), 256);
    }
}
