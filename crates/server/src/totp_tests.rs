//! Tests for `totp` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn generate_secret_returns_valid_base32() {
    let secret = generate_secret();
    assert!(!secret.is_empty());
    assert!(Secret::Encoded(secret).to_bytes().is_ok());
}

#[test]
fn build_totp_with_valid_secret() {
    let secret = generate_secret();
    assert!(build_totp(&secret, "test@example.com").is_ok());
}

#[test]
fn build_totp_with_invalid_secret() {
    assert!(build_totp("!!!invalid!!!", "test@example.com").is_err());
}

#[test]
fn verify_code_rejects_wrong_code() {
    let secret = generate_secret();
    assert!(!verify_code(&secret, "000000"));
}

#[test]
fn verify_code_accepts_current_code() {
    let secret = generate_secret();
    let totp = build_totp(&secret, "test@example.com").unwrap();
    let code = totp.generate_current().unwrap();
    assert!(verify_code(&secret, &code));
}

#[test]
fn otpauth_url_format() {
    let secret = generate_secret();
    let url = get_otpauth_url(&secret, "user@example.com", "mailrs");
    assert!(url.starts_with("otpauth://totp/mailrs:user@example.com?"));
    assert!(url.contains(&format!("secret={secret}")));
    assert!(url.contains("issuer=mailrs"));
}

#[test]
fn recovery_codes_count_and_length() {
    let codes = generate_recovery_codes();
    assert_eq!(codes.len(), 8);
    for code in &codes {
        assert_eq!(code.len(), 8);
        assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
