use chrono::{DateTime, Utc};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

/// full DB row for an API key
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ApiKeyRecord {
    pub id: i64,
    pub prefix: String,
    pub key_hash: String,
    pub account_address: String,
    pub name: String,
    pub full_key: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub app_id: Option<i64>,
}

/// lightweight struct cached in Valkey for fast auth lookups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedApiKey {
    pub key_hash: String,
    pub account_address: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub id: i64,
    /// if set, this is an app key; value is the app's internal id
    #[serde(default)]
    pub app_id: Option<i64>,
}

/// hex-encode a SHA-256 digest
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

/// generate a new API key, returning (full_key, prefix, key_hash)
///
/// format: mlrs_{8 hex prefix}_{40 hex secret}
/// total length: 5 + 8 + 1 + 40 = 54 chars
pub(crate) fn generate_api_key() -> (String, String, String) {
    let mut prefix_bytes = [0u8; 4];
    let mut secret_bytes = [0u8; 20];
    OsRng.fill_bytes(&mut prefix_bytes);
    OsRng.fill_bytes(&mut secret_bytes);

    let prefix = hex::encode(prefix_bytes);
    let secret = hex::encode(secret_bytes);
    let full_key = format!("mlrs_{prefix}_{secret}");
    let key_hash = sha256_hex(full_key.as_bytes());

    (full_key, prefix, key_hash)
}

/// insert a new API key record, returns the row id
pub(crate) async fn insert_api_key(
    pool: &PgPool,
    prefix: &str,
    key_hash: &str,
    full_key: &str,
    account_address: &str,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO api_keys (prefix, key_hash, full_key, account_address, name, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(prefix)
    .bind(key_hash)
    .bind(full_key)
    .bind(account_address)
    .bind(name)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// insert an API key for an app, returns the row id
pub(crate) async fn insert_app_api_key(
    pool: &PgPool,
    prefix: &str,
    key_hash: &str,
    full_key: &str,
    account_address: &str,
    name: &str,
    app_id: i64,
    expires_at: Option<DateTime<Utc>>,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO api_keys (prefix, key_hash, full_key, account_address, name, app_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(prefix)
    .bind(key_hash)
    .bind(full_key)
    .bind(account_address)
    .bind(name)
    .bind(app_id)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// look up an active (non-revoked) API key by prefix
pub(crate) async fn get_api_key_by_prefix(
    pool: &PgPool,
    prefix: &str,
) -> Result<Option<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "SELECT id, prefix, key_hash, account_address, name, full_key, expires_at,
                last_used_at, revoked_at, created_at, app_id
         FROM api_keys
         WHERE prefix = $1 AND revoked_at IS NULL",
    )
    .bind(prefix)
    .fetch_optional(pool)
    .await
}

/// list all active API keys for an account, newest first
pub(crate) async fn list_api_keys(
    pool: &PgPool,
    account_address: &str,
) -> Result<Vec<ApiKeyRecord>, sqlx::Error> {
    sqlx::query_as::<_, ApiKeyRecord>(
        "SELECT id, prefix, key_hash, account_address, name, full_key, expires_at,
                last_used_at, revoked_at, created_at, app_id
         FROM api_keys
         WHERE account_address = $1 AND revoked_at IS NULL
         ORDER BY created_at DESC",
    )
    .bind(account_address)
    .fetch_all(pool)
    .await
}

/// revoke an API key, returns the prefix if a row was updated (for cache eviction)
pub(crate) async fn revoke_api_key(
    pool: &PgPool,
    id: i64,
    account_address: &str,
) -> Result<Option<String>, sqlx::Error> {
    let prefix = sqlx::query_scalar::<_, String>(
        "UPDATE api_keys SET revoked_at = now()
         WHERE id = $1 AND account_address = $2 AND revoked_at IS NULL
         RETURNING prefix",
    )
    .bind(id)
    .bind(account_address)
    .fetch_optional(pool)
    .await?;

    Ok(prefix)
}

/// update last_used_at timestamp (fire-and-forget friendly)
pub(crate) async fn update_last_used(pool: &PgPool, id: i64) {
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await;
}

// --- Valkey cache helpers ---

const CACHE_TTL_SECS: u64 = 300;

/// get a cached API key from Valkey
pub(crate) async fn cache_get(
    valkey: &redis::aio::ConnectionManager,
    prefix: &str,
) -> Option<CachedApiKey> {
    let key = format!("apikey:{prefix}");
    let data: Option<String> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut valkey.clone())
        .await
        .ok()?;

    data.and_then(|s| serde_json::from_str(&s).ok())
}

/// cache an API key in Valkey with TTL
pub(crate) async fn cache_set(
    valkey: &redis::aio::ConnectionManager,
    prefix: &str,
    cached: &CachedApiKey,
) {
    let key = format!("apikey:{prefix}");
    if let Ok(json) = serde_json::to_string(cached) {
        let _: Result<(), _> = redis::cmd("SET")
            .arg(&key)
            .arg(&json)
            .arg("EX")
            .arg(CACHE_TTL_SECS)
            .query_async(&mut valkey.clone())
            .await;
    }
}

/// remove a cached API key from Valkey
pub(crate) async fn cache_delete(
    valkey: &redis::aio::ConnectionManager,
    prefix: &str,
) {
    let key = format!("apikey:{prefix}");
    let _: Result<(), _> = redis::cmd("DEL")
        .arg(&key)
        .query_async(&mut valkey.clone())
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_key_format() {
        let (full_key, prefix, key_hash) = generate_api_key();

        // format: mlrs_{8hex}_{40hex} = 54 chars total
        assert_eq!(full_key.len(), 54, "key length should be 54, got {}", full_key.len());
        assert!(full_key.starts_with("mlrs_"), "key should start with mlrs_");
        assert_eq!(prefix.len(), 8, "prefix should be 8 hex chars");
        assert_eq!(key_hash.len(), 64, "sha256 hash should be 64 hex chars");

        // verify prefix is embedded in key
        assert!(full_key.starts_with(&format!("mlrs_{prefix}_")));

        // verify hash matches
        let expected_hash = sha256_hex(full_key.as_bytes());
        assert_eq!(key_hash, expected_hash);
    }

    #[test]
    fn test_sha256_hex() {
        // known test vector: SHA-256 of empty string
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // known test vector: SHA-256 of "hello"
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_generate_key_uniqueness() {
        let mut prefixes = HashSet::new();
        let mut keys = HashSet::new();

        for _ in 0..100 {
            let (full_key, prefix, _) = generate_api_key();
            prefixes.insert(prefix);
            keys.insert(full_key);
        }

        assert_eq!(prefixes.len(), 100, "all 100 prefixes should be unique");
        assert_eq!(keys.len(), 100, "all 100 keys should be unique");
    }
}
