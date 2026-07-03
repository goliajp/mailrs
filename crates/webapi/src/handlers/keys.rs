//! `/api/mail/keys/*` + `/api/keys/{address}/*` — PGP / S/MIME
//! encryption-key list / get / set / delete + public key lookup.
//!
//! Storage layout on network kevy:
//!
//!   encryption_keys:<addr>       hash
//!     pgp                        JSON { public_key, fingerprint, created_at }
//!     smime                      JSON { public_key, fingerprint, created_at }
//!
//! Matches the monolith surface at `crates/server/src/web/mail/keys.rs`
//! bit-for-bit — only the storage backend differs.

use axum::{
    Json,
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use crate::handlers::conversations::AuthedUser;
use crate::handlers::kevy_util::with_kevy;

const MAX_PUBLIC_KEY_LEN: usize = 256 * 1024;
const MAX_FIELD_LEN: usize = 256;

#[derive(Serialize, Deserialize)]
struct StoredKey {
    public_key: String,
    fingerprint: String,
    created_at: i64,
}

#[derive(Serialize)]
pub struct EncryptionKeyInfo {
    pub key_type: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct SetKeyRequest {
    pub public_key: String,
    #[serde(default)]
    pub fingerprint: String,
}

fn validate_key_type(key_type: &str) -> bool {
    key_type == "pgp" || key_type == "smime"
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// GET /api/mail/keys — list the current user's registered keys.
/// Returns a bare array of `EncryptionKeyInfo`.
pub async fn list_keys(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
) -> Result<Json<Vec<EncryptionKeyInfo>>, StatusCode> {
    let key = format!("encryption_keys:{user}");
    let flat = with_kevy(move |c| c.hgetall(key.as_bytes()))?;
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let key_type = String::from_utf8_lossy(&flat[i]).to_string();
        if let Ok(stored) = serde_json::from_slice::<StoredKey>(&flat[i + 1]) {
            out.push(EncryptionKeyInfo {
                key_type,
                fingerprint: stored.fingerprint,
                created_at: stored.created_at,
            });
        }
        i += 2;
    }
    Ok(Json(out))
}

/// GET /api/mail/keys/{key_type} — return the current user's key of
/// this type as `{ key_type, public_key, fingerprint }`.
pub async fn get_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(key_type): Path<String>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid key_type, must be pgp or smime"})),
        );
    }
    let key = format!("encryption_keys:{user}");
    let field = key_type.clone();
    let val = match with_kevy(move |c| c.hget(key.as_bytes(), field.as_bytes())) {
        Ok(v) => v,
        Err(s) => return (s, Json(serde_json::json!({"error": "kevy error"}))),
    };
    match val.and_then(|b| serde_json::from_slice::<StoredKey>(&b).ok()) {
        Some(stored) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key_type": key_type,
                "public_key": stored.public_key,
                "fingerprint": stored.fingerprint,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
    }
}

/// PUT /api/mail/keys/{key_type} — save/replace a key.
pub async fn set_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(key_type): Path<String>,
    Json(req): Json<SetKeyRequest>,
) -> Json<serde_json::Value> {
    if !validate_key_type(&key_type) {
        return Json(serde_json::json!({"success": false, "message": "invalid key_type"}));
    }
    if req.public_key.is_empty() {
        return Json(serde_json::json!({"success": false, "message": "public_key is required"}));
    }
    if req.public_key.len() > MAX_PUBLIC_KEY_LEN {
        return Json(serde_json::json!({"success": false, "message": "public_key too large"}));
    }
    if req.fingerprint.len() > MAX_FIELD_LEN {
        return Json(serde_json::json!({"success": false, "message": "fingerprint too long"}));
    }
    let stored = StoredKey {
        public_key: req.public_key,
        fingerprint: req.fingerprint,
        created_at: now_secs(),
    };
    let json = match serde_json::to_vec(&stored) {
        Ok(v) => v,
        Err(_) => return Json(serde_json::json!({"success": false, "message": "encode error"})),
    };
    let key = format!("encryption_keys:{user}");
    let field = key_type;
    let _ = with_kevy(move |c| {
        c.hset(key.as_bytes(), &[(field.as_bytes(), json.as_slice())])?;
        Ok(())
    });
    Json(serde_json::json!({"success": true, "message": null}))
}

/// DELETE /api/mail/keys/{key_type}
pub async fn delete_key(
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Path(key_type): Path<String>,
) -> Json<serde_json::Value> {
    if !validate_key_type(&key_type) {
        return Json(serde_json::json!({"success": false, "message": "invalid key_type"}));
    }
    let key = format!("encryption_keys:{user}");
    let field = key_type;
    let _ = with_kevy(move |c| {
        c.hdel(key.as_bytes(), &[field.as_bytes()])?;
        Ok(())
    });
    Json(serde_json::json!({"success": true, "message": null}))
}

async fn get_public_key_inner(
    address: &str,
    key_type: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    if address.len() > MAX_FIELD_LEN || !address.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid address"})),
        );
    }
    if !validate_key_type(key_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid key_type"})),
        );
    }
    let key = format!("encryption_keys:{address}");
    let field = key_type.to_string();
    let val = match with_kevy(move |c| c.hget(key.as_bytes(), field.as_bytes())) {
        Ok(v) => v,
        Err(s) => return (s, Json(serde_json::json!({"error": "kevy error"}))),
    };
    match val.and_then(|b| serde_json::from_slice::<StoredKey>(&b).ok()) {
        Some(stored) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "address": address,
                "key_type": key_type,
                "public_key": stored.public_key,
                "fingerprint": stored.fingerprint,
            })),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
    }
}

/// GET /api/keys/{address}/pgp — public PGP key lookup (unauthenticated).
pub async fn get_public_pgp_key(Path(address): Path<String>) -> impl IntoResponse {
    get_public_key_inner(&address, "pgp").await
}

/// GET /api/keys/{address}/smime — public S/MIME cert lookup (unauth).
pub async fn get_public_smime_key(Path(address): Path<String>) -> impl IntoResponse {
    get_public_key_inner(&address, "smime").await
}
