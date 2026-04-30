use base64::Engine;
use crate::auth::{decode_login_response, decode_plain, AuthError};

// --- PLAIN decoding ---

#[test]
fn decode_plain_valid() {
    let (user, pass) = decode_plain("dGVzdAB0ZXN0AHBhc3M=").unwrap();
    assert_eq!(user, "test");
    assert_eq!(pass, "pass");
}

#[test]
fn decode_plain_with_authzid() {
    // format: authzid\0authcid\0passwd
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "admin\0test\0pass");
    let (user, pass) = decode_plain(&encoded).unwrap();
    assert_eq!(user, "test");
    assert_eq!(pass, "pass");
}

#[test]
fn decode_plain_empty_user() {
    // \0\0pass — empty authcid
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "\0\0pass");
    assert!(matches!(
        decode_plain(&encoded),
        Err(AuthError::EmptyUsername)
    ));
}

#[test]
fn decode_plain_empty_pass() {
    // \0user\0 — empty password
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "\0user\0");
    assert!(matches!(
        decode_plain(&encoded),
        Err(AuthError::EmptyPassword)
    ));
}

#[test]
fn decode_plain_invalid_base64() {
    assert!(matches!(
        decode_plain("not-valid-base64!!!"),
        Err(AuthError::InvalidBase64)
    ));
}

#[test]
fn decode_plain_no_null() {
    // valid base64 but no null separators
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "notnull");
    assert!(matches!(
        decode_plain(&encoded),
        Err(AuthError::MalformedPayload)
    ));
}

// --- LOGIN decoding ---

#[test]
fn decode_login_username() {
    let result = decode_login_response("dGVzdA==").unwrap();
    assert_eq!(result, "test");
}

#[test]
fn decode_login_password() {
    let result = decode_login_response("cGFzcw==").unwrap();
    assert_eq!(result, "pass");
}

#[test]
fn decode_login_invalid_base64() {
    assert!(matches!(
        decode_login_response("not-valid!!!"),
        Err(AuthError::InvalidBase64)
    ));
}

// --- decode_plain additional edge cases ---

#[test]
fn decode_plain_only_one_null() {
    // only one null separator — second null missing
    let encoded = base64::engine::general_purpose::STANDARD
        .encode("\0user");
    assert!(matches!(
        decode_plain(&encoded),
        Err(AuthError::MalformedPayload)
    ));
}

#[test]
fn decode_plain_special_chars_in_password() {
    // password contains special characters
    let payload = "\0alice\0p@$$w0rd!".to_string();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&payload);
    let (user, pass) = decode_plain(&encoded).unwrap();
    assert_eq!(user, "alice");
    assert_eq!(pass, "p@$$w0rd!");
}

#[test]
fn decode_plain_unicode_username() {
    // utf-8 username
    let payload = "\0用户\0密码".to_string();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&payload);
    let (user, pass) = decode_plain(&encoded).unwrap();
    assert_eq!(user, "用户");
    assert_eq!(pass, "密码");
}

// --- decode_login_response additional edge cases ---

#[test]
fn decode_login_empty_string_base64() {
    // empty string encoded in base64
    let encoded = base64::engine::general_purpose::STANDARD.encode("");
    let result = decode_login_response(&encoded).unwrap();
    assert_eq!(result, "");
}

#[test]
fn decode_login_special_chars() {
    let encoded = base64::engine::general_purpose::STANDARD.encode("p@$$w0rd!");
    let result = decode_login_response(&encoded).unwrap();
    assert_eq!(result, "p@$$w0rd!");
}
