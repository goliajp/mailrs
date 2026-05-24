//! Per-account PGP / S/MIME encryption-key list / get / set / delete plus
//! public key lookup endpoints.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{ApiResult, AuthUser, WebState};

/// maximum length for PGP/S/MIME public key data
const MAX_PUBLIC_KEY_LEN: usize = 256 * 1024;

#[derive(Serialize)]
pub(crate) struct EncryptionKeyInfo {
    pub key_type: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct SetKeyRequest {
    pub public_key: String,
    #[serde(default)]
    pub fingerprint: String,
}

fn validate_key_type(key_type: &str) -> bool {
    key_type == "pgp" || key_type == "smime"
}

pub(crate) async fn list_keys(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref ds) = state.domain_store else {
        return Json(serde_json::json!([]));
    };
    let rows = ds.list_encryption_keys(address).await.unwrap_or_default();
    let items: Vec<EncryptionKeyInfo> = rows
        .into_iter()
        .map(|(_, key_type, fingerprint, created_at)| EncryptionKeyInfo {
            key_type,
            fingerprint,
            created_at,
        })
        .collect();
    Json(serde_json::to_value(items).unwrap_or_default())
}

pub(crate) async fn get_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid key_type, must be pgp or smime"})),
        );
    }
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "storage unavailable"})),
        );
    };
    match ds.get_encryption_key(address, &key_type).await {
        Ok(Some((_id, public_key, fingerprint))) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "key_type": key_type,
                "public_key": public_key,
                "fingerprint": fingerprint,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

pub(crate) async fn set_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SetKeyRequest>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return Json(ApiResult {
            success: false,
            message: Some("invalid key_type, must be pgp or smime".into()),
        });
    }
    if req.public_key.is_empty() {
        return Json(ApiResult {
            success: false,
            message: Some("public_key is required".into()),
        });
    }
    if req.public_key.len() > MAX_PUBLIC_KEY_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("public_key too large".into()),
        });
    }
    if req.fingerprint.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("fingerprint too long".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("storage unavailable".into()),
        });
    };
    match ds
        .set_encryption_key(address, &key_type, &req.public_key, &req.fingerprint)
        .await
    {
        Ok(_) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

pub(crate) async fn delete_key(
    Path(key_type): Path<String>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    if !validate_key_type(&key_type) {
        return Json(ApiResult {
            success: false,
            message: Some("invalid key_type, must be pgp or smime".into()),
        });
    }
    let Some(ref ds) = state.domain_store else {
        return Json(ApiResult {
            success: false,
            message: Some("storage unavailable".into()),
        });
    };
    match ds.delete_encryption_key(address, &key_type).await {
        Ok(true) => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(false) => Json(ApiResult {
            success: false,
            message: Some("key not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}

/// public endpoint: look up anyone's PGP public key by address
pub(crate) async fn get_public_pgp_key(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    get_public_key_inner(&address, "pgp", &state).await
}

/// public endpoint: look up anyone's S/MIME certificate by address
pub(crate) async fn get_public_smime_key(
    Path(address): Path<String>,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    get_public_key_inner(&address, "smime", &state).await
}

async fn get_public_key_inner(
    address: &str,
    key_type: &str,
    state: &WebState,
) -> (StatusCode, Json<serde_json::Value>) {
    if address.len() > super::MAX_ADMIN_FIELD_LEN || !address.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid address"})),
        );
    }
    let Some(ref ds) = state.domain_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "storage unavailable"})),
        );
    };
    match ds.get_encryption_key(address, key_type).await {
        Ok(Some((_id, public_key, fingerprint))) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "address": address,
                "key_type": key_type,
                "public_key": public_key,
                "fingerprint": fingerprint,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
