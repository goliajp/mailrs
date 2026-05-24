//! Tests for `oidc_provider` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn test_pkce_s256() {
    // RFC 7636 test vector
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    assert_eq!(pkce_s256(verifier), expected);
}

#[test]
fn test_generate_auth_code_length() {
    let code = generate_auth_code();
    assert_eq!(code.len(), 64); // 32 bytes = 64 hex chars
}

#[test]
fn test_generate_auth_code_uniqueness() {
    let a = generate_auth_code();
    let b = generate_auth_code();
    assert_ne!(a, b);
}
