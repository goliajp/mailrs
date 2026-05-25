use chrono::{DateTime, Utc};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

// --- OAuth Client ---

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub(crate) struct OAuthClient {
    pub client_id: String,
    pub secret_hash: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: String,
    pub trusted: bool,
    pub active: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

/// create a new OAuth client; returns (client_id, plaintext_secret)
pub(crate) async fn create_client(
    pool: &PgPool,
    name: &str,
    redirect_uris: &[String],
    scopes: &str,
    trusted: bool,
    created_by: &str,
) -> Result<(String, String), BoxError> {
    let client_id = uuid::Uuid::new_v4().to_string();

    let mut secret_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut secret_bytes);
    let secret = hex::encode(secret_bytes);

    let secret_hash = hash_secret(&secret)?;

    sqlx::query(
        "INSERT INTO oauth_clients (client_id, secret_hash, name, redirect_uris, scopes, trusted, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&client_id)
    .bind(&secret_hash)
    .bind(name)
    .bind(redirect_uris)
    .bind(scopes)
    .bind(trusted)
    .bind(created_by)
    .execute(pool)
    .await?;

    Ok((client_id, secret))
}

/// hash a client secret with argon2
fn hash_secret(secret: &str) -> Result<String, BoxError> {
    use argon2::Argon2;
    use password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| -> BoxError { format!("argon2 hash error: {e}").into() })?;
    Ok(hash.to_string())
}

/// verify a plaintext secret against stored argon2 hash
pub(crate) fn verify_client_secret(secret: &str, hash: &str) -> bool {
    use argon2::Argon2;
    use password_hash::{PasswordHash, PasswordVerifier};
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok()
}

/// get an active client by client_id
pub(crate) async fn get_client(
    pool: &PgPool,
    client_id: &str,
) -> Result<Option<OAuthClient>, sqlx::Error> {
    sqlx::query_as::<_, OAuthClient>(
        "SELECT client_id, secret_hash, name, redirect_uris, scopes, trusted, active, created_by, created_at
         FROM oauth_clients
         WHERE client_id = $1 AND active = true",
    )
    .bind(client_id)
    .fetch_optional(pool)
    .await
}

/// list all active clients
pub(crate) async fn list_clients(pool: &PgPool) -> Result<Vec<OAuthClient>, sqlx::Error> {
    sqlx::query_as::<_, OAuthClient>(
        "SELECT client_id, secret_hash, name, redirect_uris, scopes, trusted, active, created_by, created_at
         FROM oauth_clients
         WHERE active = true
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// deactivate (soft-delete) a client
pub(crate) async fn delete_client(pool: &PgPool, client_id: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE oauth_clients SET active = false WHERE client_id = $1 AND active = true",
    )
    .bind(client_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

// --- Auth Code ---

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub(crate) struct OAuthAuthCode {
    pub code: String,
    pub client_id: String,
    pub account_address: String,
    pub redirect_uri: String,
    pub scopes: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub nonce: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    pub created_at: DateTime<Utc>,
}

/// store a new auth code
#[allow(clippy::too_many_arguments)]
pub(crate) async fn store_auth_code(
    pool: &PgPool,
    code: &str,
    client_id: &str,
    account_address: &str,
    redirect_uri: &str,
    scopes: &str,
    code_challenge: Option<&str>,
    code_challenge_method: Option<&str>,
    nonce: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_auth_codes (code, client_id, account_address, redirect_uri, scopes, code_challenge, code_challenge_method, nonce, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(code)
    .bind(client_id)
    .bind(account_address)
    .bind(redirect_uri)
    .bind(scopes)
    .bind(code_challenge)
    .bind(code_challenge_method)
    .bind(nonce)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// atomically consume an auth code (returns the code if valid and unused)
pub(crate) async fn consume_auth_code(
    pool: &PgPool,
    code: &str,
) -> Result<Option<OAuthAuthCode>, sqlx::Error> {
    sqlx::query_as::<_, OAuthAuthCode>(
        "UPDATE oauth_auth_codes SET used = true
         WHERE code = $1 AND used = false AND expires_at > now()
         RETURNING code, client_id, account_address, redirect_uri, scopes,
                   code_challenge, code_challenge_method, nonce, expires_at, used, created_at",
    )
    .bind(code)
    .fetch_optional(pool)
    .await
}

/// delete expired auth codes
pub(crate) async fn cleanup_expired_codes(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM oauth_auth_codes WHERE expires_at < now() OR used = true")
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

// --- Signing Keys ---

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub(crate) struct OAuthSigningKey {
    pub kid: String,
    pub public_key_pem: String,
    pub private_key_pem: String,
    pub algorithm: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

/// get the first active signing key
pub(crate) async fn get_active_signing_key(
    pool: &PgPool,
) -> Result<Option<OAuthSigningKey>, sqlx::Error> {
    sqlx::query_as::<_, OAuthSigningKey>(
        "SELECT kid, public_key_pem, private_key_pem, algorithm, active, created_at
         FROM oauth_signing_keys
         WHERE active = true
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// store a new signing key
pub(crate) async fn store_signing_key(
    pool: &PgPool,
    kid: &str,
    public_key_pem: &str,
    private_key_pem: &str,
    algorithm: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_signing_keys (kid, public_key_pem, private_key_pem, algorithm)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (kid) DO NOTHING",
    )
    .bind(kid)
    .bind(public_key_pem)
    .bind(private_key_pem)
    .bind(algorithm)
    .execute(pool)
    .await?;
    Ok(())
}

/// list all active public keys (for JWKS endpoint)
pub(crate) async fn list_active_public_keys(
    pool: &PgPool,
) -> Result<Vec<OAuthSigningKey>, sqlx::Error> {
    sqlx::query_as::<_, OAuthSigningKey>(
        "SELECT kid, public_key_pem, private_key_pem, algorithm, active, created_at
         FROM oauth_signing_keys
         WHERE active = true
         ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// check if any active signing key exists
pub(crate) async fn has_any_active_key(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM oauth_signing_keys WHERE active = true")
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

// --- Refresh Tokens ---

/// store a refresh token (SHA-256 hash of the plaintext token)
pub(crate) async fn store_refresh_token(
    pool: &PgPool,
    token_hash: &str,
    client_id: &str,
    account_address: &str,
    scopes: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO oauth_refresh_tokens (token_hash, client_id, account_address, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(token_hash)
    .bind(client_id)
    .bind(account_address)
    .bind(scopes)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
pub(crate) struct OAuthRefreshToken {
    pub token_hash: String,
    pub client_id: String,
    pub account_address: String,
    pub scopes: String,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
}

/// validate a refresh token (by its SHA-256 hash)
pub(crate) async fn validate_refresh_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<OAuthRefreshToken>, sqlx::Error> {
    sqlx::query_as::<_, OAuthRefreshToken>(
        "SELECT token_hash, client_id, account_address, scopes, expires_at, revoked, created_at
         FROM oauth_refresh_tokens
         WHERE token_hash = $1 AND revoked = false AND expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// revoke a refresh token
pub(crate) async fn revoke_refresh_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE oauth_refresh_tokens SET revoked = true WHERE token_hash = $1 AND revoked = false",
    )
    .bind(token_hash)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// delete expired or revoked refresh tokens
pub(crate) async fn cleanup_expired_refresh_tokens(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM oauth_refresh_tokens WHERE expires_at < now() OR revoked = true")
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}

/// helper: SHA-256 hex digest (reuse pattern from api_key_store)
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

/// generate a random hex token of the given byte length
pub(crate) fn generate_random_hex(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
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
}
