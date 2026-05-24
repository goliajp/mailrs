//! Tests for `oidc_jwt` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn test_current_kid_format() {
    let kid = current_kid();
    assert!(kid.starts_with("key-"));
    // format: key-YYYY-MM (4 + 7 = 11 chars)
    assert_eq!(kid.len(), 11);
}

#[tokio::test]
async fn test_generate_keypair_and_sign_verify() {
    let (private_pem, public_pem) = generate_rsa_keypair().await.unwrap();
    assert!(private_pem.contains("PRIVATE KEY"));
    assert!(public_pem.contains("PUBLIC KEY"));

    let claims = serde_json::json!({
        "sub": "test@example.com",
        "iss": "https://mail.example.com",
        "iat": chrono::Utc::now().timestamp(),
        "exp": chrono::Utc::now().timestamp() + 300,
    });

    let token = sign_jwt(&private_pem, "test-kid", &claims).unwrap();
    assert!(!token.is_empty());

    // verify
    let decoded = verify_jwt(&token, &public_pem).unwrap();
    assert_eq!(decoded["sub"], "test@example.com");
    assert_eq!(decoded["iss"], "https://mail.example.com");
}

#[tokio::test]
async fn test_pem_to_jwk() {
    let (_, public_pem) = generate_rsa_keypair().await.unwrap();
    let jwk = pem_to_jwk(&public_pem, "test-kid").unwrap();

    assert_eq!(jwk["kty"], "RSA");
    assert_eq!(jwk["use"], "sig");
    assert_eq!(jwk["alg"], "RS256");
    assert_eq!(jwk["kid"], "test-kid");
    assert!(jwk["n"].as_str().unwrap().len() > 10);
    assert!(!jwk["e"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_sign_jwt_invalid_key() {
    let claims = serde_json::json!({"sub": "test"});
    let result = sign_jwt("not a valid pem", "kid", &claims);
    assert!(result.is_err());
}

#[test]
fn test_verify_jwt_invalid_token() {
    let result = verify_jwt("not.a.jwt", "not a valid pem");
    assert!(result.is_err());
}
