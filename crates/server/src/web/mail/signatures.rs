//! Per-account signature list / create / update / delete.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::{ApiResult, AuthUser, WebState};

#[derive(Serialize)]
pub(crate) struct SignatureInfo {
    pub id: i64,
    pub name: String,
    pub html: String,
    pub text_content: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Deserialize)]
pub(crate) struct SaveSignatureRequest {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default = "default_signature_name")]
    pub name: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub text_content: String,
    #[serde(default)]
    pub is_default: bool,
}

fn default_signature_name() -> String {
    "Default".into()
}

pub(crate) async fn list_signatures(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(serde_json::json!([]));
    };
    let rows = sqlx::query_as::<_, (i64, String, String, String, bool, String)>(
        "SELECT id, name, html, text_content, is_default, \
         to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM signatures WHERE account_address = $1 ORDER BY created_at",
    )
    .bind(address)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let sigs: Vec<SignatureInfo> = rows
        .into_iter()
        .map(
            |(id, name, html, text_content, is_default, created_at)| SignatureInfo {
                id,
                name,
                html,
                text_content,
                is_default,
                created_at,
            },
        )
        .collect();
    Json(serde_json::to_value(sigs).unwrap_or_default())
}

pub(crate) async fn save_signature(
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SaveSignatureRequest>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database unavailable".into()),
        });
    };
    if req.name.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(ApiResult {
            success: false,
            message: Some("signature name too long".into()),
        });
    }
    if req.html.len() > super::MAX_EMAIL_BODY_LEN
        || req.text_content.len() > super::MAX_EMAIL_BODY_LEN
    {
        return Json(ApiResult {
            success: false,
            message: Some("signature content too long".into()),
        });
    }

    // if setting as default, unset any existing default first
    if req.is_default {
        let _ = sqlx::query("UPDATE signatures SET is_default = false WHERE account_address = $1")
            .bind(address)
            .execute(pool)
            .await;
    }

    let result = if let Some(id) = req.id {
        // update existing
        sqlx::query(
            "UPDATE signatures SET name = $1, html = $2, text_content = $3, is_default = $4 \
             WHERE id = $5 AND account_address = $6",
        )
        .bind(&req.name)
        .bind(&req.html)
        .bind(&req.text_content)
        .bind(req.is_default)
        .bind(id)
        .bind(address)
        .execute(pool)
        .await
    } else {
        // insert new
        sqlx::query(
            "INSERT INTO signatures (account_address, name, html, text_content, is_default) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(address)
        .bind(&req.name)
        .bind(&req.html)
        .bind(&req.text_content)
        .bind(req.is_default)
        .execute(pool)
        .await
    };

    match result {
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

pub(crate) async fn delete_signature(
    Path(id): Path<i64>,
    AuthUser { ref address, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database unavailable".into()),
        });
    };
    let result = sqlx::query("DELETE FROM signatures WHERE id = $1 AND account_address = $2")
        .bind(id)
        .bind(address)
        .execute(pool)
        .await;
    match result {
        Ok(r) if r.rows_affected() > 0 => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(_) => Json(ApiResult {
            success: false,
            message: Some("signature not found".into()),
        }),
        Err(e) => Json(ApiResult {
            success: false,
            message: Some(e.to_string()),
        }),
    }
}
