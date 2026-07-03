//! Email templates CRUD — direct SQL through `state.pool`. Same pattern
//! as `drafts.rs` / `signatures.rs`. Does NOT touch the existing handler
//! at `crates/server/src/web/templates.rs` (ironrule).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// GET /v1/users/{user}/templates
pub async fn list_templates(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
) -> Result<Json<wire::TemplateListResponse>, StatusCode> {
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            String,
            String,
            bool,
            String,
            String,
        ),
    >(
        "SELECT id, name, subject, html_body, text_body, category, is_default, \
                to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
         FROM email_templates WHERE user_address = $1 \
         ORDER BY updated_at DESC",
    )
    .bind(&user)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "list_templates failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let items = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                subject,
                html_body,
                text_body,
                category,
                is_default,
                created_at,
                updated_at,
            )| wire::TemplateWire {
                id,
                name,
                subject,
                html_body,
                text_body,
                category,
                is_default,
                created_at,
                updated_at,
            },
        )
        .collect();
    Ok(Json(wire::TemplateListResponse { items }))
}

/// POST /v1/users/{user}/templates  — upsert by (user_address, name)
pub async fn save_template(
    State(state): State<Arc<CoreRpcState>>,
    Path(user): Path<String>,
    Json(req): Json<wire::SaveTemplateRequest>,
) -> Result<Json<wire::SaveTemplateResponse>, StatusCode> {
    if req.name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let category = if req.category.is_empty() {
        "general".to_string()
    } else {
        req.category.clone()
    };

    if req.is_default {
        sqlx::query(
            "UPDATE email_templates SET is_default = false \
             WHERE user_address = $1 AND is_default = true",
        )
        .bind(&user)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, "save_template unset-default failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO email_templates \
             (user_address, name, subject, html_body, text_body, category, is_default, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
         ON CONFLICT (user_address, name) DO UPDATE SET \
             subject = EXCLUDED.subject, \
             html_body = EXCLUDED.html_body, \
             text_body = EXCLUDED.text_body, \
             category = EXCLUDED.category, \
             is_default = EXCLUDED.is_default, \
             updated_at = now() \
         RETURNING id",
    )
    .bind(&user)
    .bind(&req.name)
    .bind(&req.subject)
    .bind(&req.html_body)
    .bind(&req.text_body)
    .bind(&category)
    .bind(req.is_default)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, user = %user, "save_template upsert failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::SaveTemplateResponse { id }))
}

/// DELETE /v1/users/{user}/templates/{id}
pub async fn delete_template(
    State(state): State<Arc<CoreRpcState>>,
    Path((user, id)): Path<(String, i64)>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query("DELETE FROM email_templates WHERE id = $1 AND user_address = $2")
        .bind(id)
        .bind(&user)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, user = %user, id, "delete_template failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
