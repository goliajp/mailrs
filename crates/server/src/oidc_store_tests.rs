//! Tests for `oidc_store` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn test_sha256_hex() {
    let hash = sha256_hex(b"test");
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash,
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    );
}

#[test]
fn test_generate_random_hex_length() {
    let token = generate_random_hex(32);
    assert_eq!(token.len(), 64);
    let token2 = generate_random_hex(16);
    assert_eq!(token2.len(), 32);
}

#[test]
fn test_generate_random_hex_uniqueness() {
    let a = generate_random_hex(32);
    let b = generate_random_hex(32);
    assert_ne!(a, b);
}

#[test]
fn test_hash_and_verify_secret() {
    let secret = "test-secret-value";
    let hash = hash_secret(secret).expect("hash should succeed");
    assert!(verify_client_secret(secret, &hash));
    assert!(!verify_client_secret("wrong-secret", &hash));
}
