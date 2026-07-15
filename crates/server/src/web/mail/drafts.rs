//! Draft create / list / delete.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::{ApiResult, AuthUser, WebState};

#[derive(Deserialize)]
pub(crate) struct SaveDraftRequest {
    /// When present, upsert that draft in place (keeps its id) instead of
    /// inserting a new row — so a compose session's periodic autosave
    /// updates one draft rather than spawning a new one each tick.
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub reply_to_thread_id: Option<String>,
}

// wire shape matches fastcore's `DraftWire` (to/cc/bcc + epoch-second
// timestamps) so the frontend `draftSchema` parses identically on both
// lanes — core-mode parity.
#[derive(Serialize)]
pub(crate) struct DraftInfo {
    pub id: i64,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    pub reply_to_thread_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize)]
pub(crate) struct SaveDraftResult {
    pub success: bool,
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub(crate) async fn save_draft(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<SaveDraftRequest>,
) -> impl IntoResponse {
    if req.body.len() > super::MAX_EMAIL_BODY_LEN {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("draft body too large".into()),
        });
    }
    if req.subject.len() > super::MAX_ADMIN_FIELD_LEN {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("subject too long".into()),
        });
    }

    let Some(ref pool) = state.pg_pool else {
        return Json(SaveDraftResult {
            success: false,
            id: None,
            message: Some("database not configured".into()),
        });
    };

    // upsert: an id updates that row in place (scoped to the user); a
    // missing/unknown id inserts a fresh row. an id that no longer exists
    // (draft deleted meanwhile) falls through to an insert so autosave
    // keeps working.
    let mut updated_id: Option<i64> = None;
    if let Some(id) = req.id {
        updated_id = sqlx::query_scalar::<_, i64>(
            "UPDATE drafts SET to_addresses = $3, cc_addresses = $4, bcc_addresses = $5, \
             subject = $6, body = $7, reply_to_thread_id = $8, updated_at = now() \
             WHERE id = $1 AND user_address = $2 RETURNING id",
        )
        .bind(id)
        .bind(&user)
        .bind(&req.to)
        .bind(&req.cc)
        .bind(&req.bcc)
        .bind(&req.subject)
        .bind(&req.body)
        .bind(&req.reply_to_thread_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);
    }

    let result = match updated_id {
        Some(id) => Ok(id),
        None => {
            sqlx::query_scalar::<_, i64>(
                "INSERT INTO drafts (user_address, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 RETURNING id",
            )
            .bind(&user)
            .bind(&req.to)
            .bind(&req.cc)
            .bind(&req.bcc)
            .bind(&req.subject)
            .bind(&req.body)
            .bind(&req.reply_to_thread_id)
            .fetch_one(pool)
            .await
        }
    };

    match result {
        Ok(id) => Json(SaveDraftResult {
            success: true,
            id: Some(id),
            message: None,
        }),
        Err(e) => {
            tracing::error!(event = "draft_save_failed", error = %e);
            Json(SaveDraftResult {
                success: false,
                id: None,
                message: Some("failed to save draft".into()),
            })
        }
    }
}

pub(crate) async fn list_drafts(
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(Vec::<DraftInfo>::new());
    };

    let rows = sqlx::query_as::<_, (i64, String, String, String, String, String, Option<String>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
        "SELECT id, to_addresses, cc_addresses, bcc_addresses, subject, body, reply_to_thread_id, created_at, updated_at \
         FROM drafts WHERE user_address = $1 ORDER BY updated_at DESC",
    )
    .bind(&user)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let drafts: Vec<DraftInfo> = rows
        .into_iter()
        .map(
            |(
                id,
                to_addresses,
                cc_addresses,
                bcc_addresses,
                subject,
                body,
                reply_to_thread_id,
                created_at,
                updated_at,
            )| {
                DraftInfo {
                    id,
                    to: to_addresses,
                    cc: cc_addresses,
                    bcc: bcc_addresses,
                    subject,
                    body,
                    reply_to_thread_id,
                    created_at: created_at.timestamp(),
                    updated_at: updated_at.timestamp(),
                }
            },
        )
        .collect();

    Json(drafts)
}

pub(crate) async fn delete_draft(
    Path(id): Path<i64>,
    AuthUser { address: user, .. }: AuthUser,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    let Some(ref pool) = state.pg_pool else {
        return Json(ApiResult {
            success: false,
            message: Some("database not configured".into()),
        });
    };

    let result = sqlx::query("DELETE FROM drafts WHERE id = $1 AND user_address = $2")
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
            message: Some("draft not found".into()),
        }),
        Err(e) => {
            tracing::error!(event = "draft_delete_failed", error = %e);
            Json(ApiResult {
                success: false,
                message: Some("failed to delete draft".into()),
            })
        }
    }
}
