use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsa::pkcs1::EncodeRsaPublicKey;
use rsa::pkcs8::EncodePrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sqlx::PgPool;

use crate::oidc_store;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// generate an RSA 2048-bit keypair, returning (private_pem, public_pem)
pub(crate) async fn generate_rsa_keypair() -> Result<(String, String), BoxError> {
    // RSA key generation is CPU-intensive; run in a blocking thread
    let (priv_key, pub_key) = tokio::task::spawn_blocking(|| {
        let priv_key = RsaPrivateKey::new(&mut rand_core::OsRng, 2048)?;
        let pub_key = RsaPublicKey::from(&priv_key);
        Ok::<_, rsa::Error>((priv_key, pub_key))
    })
    .await
    .map_err(|e| -> BoxError { format!("spawn_blocking join error: {e}").into() })??;

    let private_pem = priv_key
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(|e| -> BoxError { format!("pkcs8 encode error: {e}").into() })?
        .to_string();

    let public_pem = pub_key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .map_err(|e| -> BoxError { format!("pkcs1 encode error: {e}").into() })?
        .to_string();

    Ok((private_pem, public_pem))
}

/// compute the current key id based on year-month
pub(crate) fn current_kid() -> String {
    let now = chrono::Utc::now();
    format!("key-{}", now.format("%Y-%m"))
}

/// ensure at least one active signing key exists; generate if needed
pub(crate) async fn ensure_signing_key(pool: &PgPool) -> Result<(), BoxError> {
    if oidc_store::has_any_active_key(pool).await? {
        tracing::info!("oidc signing key already exists");
        return Ok(());
    }

    tracing::info!("generating new oidc signing key");
    let kid = current_kid();
    let (private_pem, public_pem) = generate_rsa_keypair().await?;
    oidc_store::store_signing_key(pool, &kid, &public_pem, &private_pem, "RS256").await?;
    tracing::info!(kid = kid.as_str(), "oidc signing key generated");
    Ok(())
}

/// sign a JWT with RS256 using the given private key PEM
pub(crate) fn sign_jwt(
    private_key_pem: &str,
    kid: &str,
    claims: &serde_json::Value,
) -> Result<String, BoxError> {
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
        .map_err(|e| -> BoxError { format!("invalid rsa pem for signing: {e}").into() })?;

    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_string());

    let token = jsonwebtoken::encode(&header, claims, &key)
        .map_err(|e| -> BoxError { format!("jwt sign error: {e}").into() })?;

    Ok(token)
}

/// convert an RSA public key PEM to a JWK JSON object
pub(crate) fn pem_to_jwk(
    public_key_pem: &str,
    kid: &str,
) -> Result<serde_json::Value, BoxError> {
    use rsa::pkcs1::DecodeRsaPublicKey;

    let pub_key = RsaPublicKey::from_pkcs1_pem(public_key_pem)
        .map_err(|e| -> BoxError { format!("parse rsa public key pem: {e}").into() })?;

    let n_bytes = pub_key.n().to_bytes_be();
    let e_bytes = pub_key.e().to_bytes_be();

    let n_b64 = URL_SAFE_NO_PAD.encode(&n_bytes);
    let e_b64 = URL_SAFE_NO_PAD.encode(&e_bytes);

    Ok(serde_json::json!({
        "kty": "RSA",
        "use": "sig",
        "alg": "RS256",
        "kid": kid,
        "n": n_b64,
        "e": e_b64,
    }))
}

/// decode and verify a JWT against the given public key PEM, returning the claims
pub(crate) fn verify_jwt(
    token: &str,
    public_key_pem: &str,
) -> Result<serde_json::Value, BoxError> {
    let key = jsonwebtoken::DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
        .map_err(|e| -> BoxError { format!("invalid rsa pem for verification: {e}").into() })?;

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    // we validate expiration but skip audience check (caller may do it)
    validation.validate_aud = false;

    let data = jsonwebtoken::decode::<serde_json::Value>(token, &key, &validation)
        .map_err(|e| -> BoxError { format!("jwt verify error: {e}").into() })?;

    Ok(data.claims)
}

#[cfg(test)]
#[path = "oidc_jwt_tests.rs"]
mod tests;
