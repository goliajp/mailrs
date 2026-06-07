use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api_key_store;

use super::WebState;
use super::auth::{AuthMethod, AuthUser};

#[derive(Deserialize)]
pub(super) struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CreateApiKeyResponse {
    id: i64,
    key: String,
    prefix: String,
    name: String,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct ApiKeyResponse {
    id: i64,
    key: Option<String>,
    prefix: String,
    name: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
}

/// POST /api/agent/keys — create a new API key (session auth only)
pub(super) async fn create_api_key(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    // only session-authenticated users can create API keys
    if matches!(auth_user.auth_method, AuthMethod::ApiKey(_)) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "api keys cannot create other api keys"})),
        );
    }

    // validate name
    if req.name.is_empty() || req.name.len() > 100 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "name must be 1-100 characters"})),
        );
    }

    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        }
    };

    // retry up to 3 times on prefix collision
    for _ in 0..3 {
        let (full_key, prefix, key_hash) = api_key_store::generate_api_key();

        match api_key_store::insert_api_key(
            pool,
            &prefix,
            &key_hash,
            &full_key,
            &auth_user.address,
            &req.name,
            req.expires_at,
        )
        .await
        {
            Ok(id) => {
                return (
                    StatusCode::CREATED,
                    Json(serde_json::json!(CreateApiKeyResponse {
                        id,
                        key: full_key,
                        prefix,
                        name: req.name,
                        expires_at: req.expires_at,
                    })),
                );
            }
            Err(e) => {
                // check for unique constraint violation (prefix collision)
                let msg = e.to_string();
                if msg.contains("unique") || msg.contains("duplicate") {
                    continue;
                }
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "failed to create api key"})),
                );
            }
        }
    }

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "failed to generate unique key prefix"})),
    )
}

/// GET /api/agent/keys — list all active API keys for the authenticated user
pub(super) async fn list_api_keys(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
) -> impl IntoResponse {
    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        }
    };

    match api_key_store::list_api_keys(pool, &auth_user.address).await {
        Ok(records) => {
            let keys: Vec<ApiKeyResponse> = records
                .into_iter()
                .map(|r| ApiKeyResponse {
                    id: r.id,
                    key: r.full_key,
                    prefix: r.prefix,
                    name: r.name,
                    created_at: r.created_at,
                    expires_at: r.expires_at,
                    last_used_at: r.last_used_at,
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!(keys)))
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to list api keys"})),
        ),
    }
}

/// DELETE /api/agent/keys/{id} — revoke an API key
pub(super) async fn revoke_api_key(
    State(state): State<Arc<WebState>>,
    auth_user: AuthUser,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let pool = match state.pg_pool.as_ref() {
        Some(p) => p,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "database unavailable"})),
            );
        }
    };

    match api_key_store::revoke_api_key(pool, id, &auth_user.address).await {
        Ok(Some(_prefix)) => {
            // No cache to evict — verify always hits PG with
            // `revoked_at IS NULL` filter, so the revoke is effective
            // immediately.
            (StatusCode::OK, Json(serde_json::json!({"success": true})))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "api key not found or already revoked"})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "failed to revoke api key"})),
        ),
    }
}

#[cfg(test)]
mod tests {
    use crate::api_key_store::{self, VerifiedApiKey};

    use super::*;

    // --- key format tests (pure unit, no DB) ---

    #[test]
    fn test_key_format_valid() {
        let (full_key, prefix, _key_hash) = api_key_store::generate_api_key();

        assert!(full_key.starts_with("mlrs_"), "key should start with mlrs_");
        assert_eq!(full_key.len(), 54, "key length should be 54");

        let parts: Vec<&str> = full_key.splitn(3, '_').collect();
        assert_eq!(parts.len(), 3, "key should have 3 parts separated by _");
        assert_eq!(parts[0], "mlrs");
        assert_eq!(parts[1].len(), 8, "prefix should be 8 hex chars");
        assert_eq!(parts[2].len(), 40, "secret should be 40 hex chars");
        assert_eq!(prefix, parts[1]);
    }

    #[test]
    fn test_key_hash_is_sha256() {
        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let computed = api_key_store::sha256_hex(full_key.as_bytes());
        assert_eq!(
            key_hash, computed,
            "stored hash should match sha256 of full key"
        );
    }

    // --- auth logic tests ---

    /// helper to test verify logic without hitting DB/Kevy
    fn verify_api_key_logic(
        token: &str,
        cached: Option<&VerifiedApiKey>,
    ) -> Result<AuthUser, (StatusCode, &'static str)> {
        // parse token
        let parts: Vec<&str> = token.splitn(3, '_').collect();
        if parts.len() != 3 || parts[0] != "mlrs" {
            return Err((StatusCode::UNAUTHORIZED, "invalid api key format"));
        }

        let cached = cached.ok_or((StatusCode::UNAUTHORIZED, "invalid api key"))?;

        // verify hash
        let token_hash = api_key_store::sha256_hex(token.as_bytes());
        if token_hash != cached.key_hash {
            return Err((StatusCode::UNAUTHORIZED, "invalid api key"));
        }

        // check expiration
        if let Some(expires_at) = cached.expires_at
            && expires_at < Utc::now()
        {
            return Err((StatusCode::UNAUTHORIZED, "api key expired"));
        }

        // permissions are loaded from domain_store at runtime, not from cache
        let perms = crate::permission::compute_effective_permissions(&[], &[], &[]);
        Ok(AuthUser {
            address: cached.account_address.clone(),
            display_name: cached.account_address.clone(),
            permissions: std::sync::Arc::new(perms),
            auth_method: AuthMethod::ApiKey(cached.id),
        })
    }

    #[test]
    fn test_bearer_auth_works() {
        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = VerifiedApiKey {
            key_hash,
            account_address: "user@example.com".to_string(),
            expires_at: None,
            id: 42,
            app_id: None,
        };

        let result = verify_api_key_logic(&full_key, Some(&cached));
        assert!(result.is_ok(), "valid key should authenticate");

        let user = result.unwrap();
        assert_eq!(user.address, "user@example.com");
        assert!(matches!(user.auth_method, AuthMethod::ApiKey(42)));
    }

    #[test]
    fn test_expired_key_rejected() {
        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = VerifiedApiKey {
            key_hash,
            account_address: "user@example.com".to_string(),
            expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
            id: 1,
            app_id: None,
        };

        let result = verify_api_key_logic(&full_key, Some(&cached));
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(msg, "api key expired");
    }

    #[test]
    fn test_wrong_key_secret() {
        let (_full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = VerifiedApiKey {
            key_hash,
            account_address: "user@example.com".to_string(),
            expires_at: None,
            id: 1,
            app_id: None,
        };

        // use a different key (same format but different secret)
        let (other_key, _, _) = api_key_store::generate_api_key();
        let result = verify_api_key_logic(&other_key, Some(&cached));
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(msg, "invalid api key");
    }

    #[test]
    fn test_invalid_key_format() {
        let result = verify_api_key_logic("not_a_key", None);
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(msg, "invalid api key format");
    }

    #[test]
    fn test_invalid_key_format_no_prefix() {
        let result = verify_api_key_logic("bearer_something", None);
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(msg, "invalid api key format");
    }

    #[test]
    fn test_api_key_produces_auth_user() {
        let (full_key, _prefix, key_hash) = api_key_store::generate_api_key();
        let cached = VerifiedApiKey {
            key_hash,
            account_address: "admin@golia.jp".to_string(),
            expires_at: None,
            id: 99,
            app_id: None,
        };

        let result = verify_api_key_logic(&full_key, Some(&cached));
        assert!(result.is_ok());

        let user = result.unwrap();
        assert_eq!(user.address, "admin@golia.jp");
        assert!(matches!(user.auth_method, AuthMethod::ApiKey(99)));
    }

    // revoke tested via manual integration: POST create -> DELETE revoke -> GET with key -> 401
}
