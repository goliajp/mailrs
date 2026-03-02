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
