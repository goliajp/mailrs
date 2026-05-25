use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::{ApiResult, AuthUser, WebState};

#[derive(Deserialize)]
pub(super) struct SaveTemplateRequest {
    pub name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub html_body: String,
    #[serde(default)]
    pub text_body: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub is_default: bool,
}

fn default_category() -> String {
    "general".into()
}

#[derive(Serialize)]
pub(super) struct TemplateInfo {
    pub id: i64,
    pub name: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
    pub category: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub(super) struct SaveTemplateResult {
    pub success: bool,
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub(super) async fn save_template(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SaveTemplateRequest>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(SaveTemplateResult {
            success: false,
            id: None,
            message: Some("database unavailable".into()),
        });
    };

    if req.name.trim().is_empty() {
        return Json(SaveTemplateResult {
            success: false,
            id: None,
            message: Some("name is required".into()),
        });
    }

    // if setting this template as default, clear other defaults first
    // (partial unique index enforces at most one default per user)
    if req.is_default {
        let _ = sqlx::query(
            "UPDATE email_templates SET is_default = false WHERE user_address = $1 AND is_default = true",
        )
        .bind(&user)
        .execute(pool)
        .await;
    }

    let result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO email_templates (user_address, name, subject, html_body, text_body, category, is_default, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT (user_address, name) DO UPDATE SET
           subject = EXCLUDED.subject,
           html_body = EXCLUDED.html_body,
           text_body = EXCLUDED.text_body,
           category = EXCLUDED.category,
           is_default = EXCLUDED.is_default,
           updated_at = now()
         RETURNING id",
    )
    .bind(&user)
    .bind(req.name.trim())
    .bind(&req.subject)
    .bind(&req.html_body)
    .bind(&req.text_body)
    .bind(&req.category)
    .bind(req.is_default)
    .fetch_one(pool)
    .await;

    match result {
        Ok(id) => Json(SaveTemplateResult {
            success: true,
            id: Some(id),
            message: None,
        }),
        Err(e) => {
            // log full error server-side but avoid leaking schema details to clients
            tracing::error!(event = "template_save_failed", error = %e);
            Json(SaveTemplateResult {
                success: false,
                id: None,
                message: Some("failed to save template".into()),
            })
        }
    }
}

pub(super) async fn list_templates(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<TemplateInfo>::new());
    };

    let rows =
        match sqlx::query_as::<_, (i64, String, String, String, String, String, bool, i64, i64)>(
            "SELECT id, name, subject, html_body, text_body, category, is_default,
                EXTRACT(EPOCH FROM created_at)::bigint,
                EXTRACT(EPOCH FROM updated_at)::bigint
         FROM email_templates WHERE user_address = $1
         ORDER BY is_default DESC, updated_at DESC",
        )
        .bind(&user)
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!(event = "template_list_failed", user = %user, error = %e);
                return Json(Vec::new());
            }
        };

    Json(
        rows.into_iter()
            .map(|r| TemplateInfo {
                id: r.0,
                name: r.1,
                subject: r.2,
                html_body: r.3,
                text_body: r.4,
                category: r.5,
                is_default: r.6,
                created_at: r.7.to_string(),
                updated_at: r.8.to_string(),
            })
            .collect::<Vec<_>>(),
    )
}

pub(super) async fn delete_template(
    Path(id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database unavailable".into()),
        });
    };

    let result = sqlx::query("DELETE FROM email_templates WHERE id = $1 AND user_address = $2")
        .bind(id)
        .bind(&user)
        .execute(pool)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => Json(ApiResult {
            success: true,
            message: None,
        }),
        Ok(_) => Json(ApiResult {
            success: false,
            message: Some("template not found".into()),
        }),
        Err(e) => {
            tracing::error!(event = "template_delete_failed", error = %e);
            Json(ApiResult {
                success: false,
                message: Some("failed to delete template".into()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_category_is_general() {
        assert_eq!(default_category(), "general");
    }

    #[test]
    fn save_request_deserialize_minimal() {
        let json = r#"{"name":"test"}"#;
        let req: SaveTemplateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test");
        assert_eq!(req.subject, "");
        assert_eq!(req.category, "general");
        assert!(!req.is_default);
    }

    #[test]
    fn save_request_deserialize_full() {
        let json = r#"{"name":"meeting","subject":"Meeting Invite","html_body":"<p>Hi</p>","text_body":"Hi","category":"work","is_default":true}"#;
        let req: SaveTemplateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "meeting");
        assert_eq!(req.subject, "Meeting Invite");
        assert_eq!(req.category, "work");
        assert!(req.is_default);
    }
}
