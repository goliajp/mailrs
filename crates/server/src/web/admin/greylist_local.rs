//! Admin endpoints for Phase 2 greylist local white/black lists.
//!
//! - `GET    /api/admin/greylist/local-lists`           — list (optional `kind`, `list` filters)
//! - `POST   /api/admin/greylist/local-lists`           — create one entry
//! - `DELETE /api/admin/greylist/local-lists/:id`       — remove by id
//!
//! All endpoints require the `admin.greylist` permission.
//!
//! Writes always reload the in-memory snapshot inline so the next inbound
//! mail honors the change without waiting for the periodic refresher.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::*;
use crate::greylist_local::{normalize, validate_list};

#[derive(Serialize)]
pub(crate) struct GreylistLocalEntry {
    pub id: i64,
    pub kind: String,
    pub list: String,
    pub value: String,
    pub note: Option<String>,
    pub created_at: i64,
    pub created_by: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct ListQuery {
    pub kind: Option<String>,
    pub list: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CreateRequest {
    pub kind: String,
    pub list: String,
    pub value: String,
    pub note: Option<String>,
}

#[derive(Serialize)]
struct CreatedResponse {
    id: i64,
    kind: String,
    list: String,
    value: String,
}

fn err400(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

fn err403() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": "admin.greylist permission required" })),
    )
}

fn err409(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

fn err500(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

fn err503(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": message.into() })),
    )
}

pub(crate) async fn list(
    AuthUser {
        ref permissions, ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    if !permissions.has("admin.greylist") {
        return err403().into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return err503("postgres not configured").into_response();
    };
    let mut sql = "SELECT id, kind, list, value, note, \
         EXTRACT(EPOCH FROM created_at)::bigint, created_by \
         FROM greylist_local_lists WHERE 1=1"
        .to_string();
    let mut binds: Vec<String> = Vec::new();
    if let Some(ref k) = q.kind
        && matches!(k.as_str(), "domain" | "email" | "cidr")
    {
        binds.push(k.clone());
        sql.push_str(&format!(" AND kind = ${}", binds.len()));
    }
    if let Some(ref l) = q.list
        && matches!(l.as_str(), "white" | "black")
    {
        binds.push(l.clone());
        sql.push_str(&format!(" AND list = ${}", binds.len()));
    }
    sql.push_str(" ORDER BY id");
    let mut query = sqlx::query_as::<_, (i64, String, String, String, Option<String>, i64, Option<String>)>(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    match query.fetch_all(pool).await {
        Ok(rows) => {
            let entries: Vec<GreylistLocalEntry> = rows
                .into_iter()
                .map(
                    |(id, kind, list, value, note, created_at, created_by)| GreylistLocalEntry {
                        id,
                        kind,
                        list,
                        value,
                        note,
                        created_at,
                        created_by,
                    },
                )
                .collect();
            Json(entries).into_response()
        }
        Err(e) => err500(format!("query failed: {e}")).into_response(),
    }
}

pub(crate) async fn create(
    AuthUser {
        ref permissions,
        ref address,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Json(req): Json<CreateRequest>,
) -> impl IntoResponse {
    if !permissions.has("admin.greylist") {
        return err403().into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return err503("postgres not configured").into_response();
    };

    if let Err(e) = validate_list(&req.list) {
        return err400(e.to_string()).into_response();
    }
    let normalized = match normalize(&req.kind, &req.value) {
        Ok(v) => v,
        Err(e) => return err400(e.to_string()).into_response(),
    };

    // INSERT and let PG enforce the (kind, value) uniqueness. On conflict
    // surface a helpful 409 that names the existing row's id + list, so
    // the operator knows whether to DELETE first or just accept the state.
    let row: Result<(i64,), sqlx::Error> = sqlx::query_as(
        "INSERT INTO greylist_local_lists (kind, list, value, note, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(&req.kind)
    .bind(&req.list)
    .bind(&normalized)
    .bind(req.note.as_deref())
    .bind(address.as_str())
    .fetch_one(pool)
    .await;

    match row {
        Ok((id,)) => {
            crate::greylist_local::reload(&state.greylist_local, pool).await;
            tracing::info!(
                target: "admin.greylist",
                id,
                kind = %req.kind,
                list = %req.list,
                value = %normalized,
                actor = %address,
                "greylist_local entry added"
            );
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(CreatedResponse {
                    id,
                    kind: req.kind,
                    list: req.list,
                    value: normalized,
                })
                .unwrap_or(serde_json::Value::Null)),
            )
                .into_response()
        }
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            // pull the existing row so the message is actually useful
            let existing: Option<(i64, String)> = sqlx::query_as(
                "SELECT id, list FROM greylist_local_lists WHERE kind = $1 AND value = $2",
            )
            .bind(&req.kind)
            .bind(&normalized)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
            let msg = match existing {
                Some((id, list)) => format!(
                    "value '{normalized}' already exists on the {list} list (id={id}); \
                     delete it first to move it to a different list"
                ),
                None => format!("value '{normalized}' already exists"),
            };
            err409(msg).into_response()
        }
        Err(e) => {
            tracing::warn!(error = %e, "greylist_local insert failed");
            err500(format!("insert failed: {e}")).into_response()
        }
    }
}

pub(crate) async fn remove(
    AuthUser {
        ref permissions,
        ref address,
        ..
    }: AuthUser,
    State(state): State<Arc<WebState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if !permissions.has("admin.greylist") {
        return err403().into_response();
    }
    let Some(ref pool) = state.pg_pool else {
        return err503("postgres not configured").into_response();
    };
    let res = sqlx::query("DELETE FROM greylist_local_lists WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await;
    match res {
        Ok(r) if r.rows_affected() > 0 => {
            crate::greylist_local::reload(&state.greylist_local, pool).await;
            tracing::info!(
                target: "admin.greylist",
                id,
                actor = %address,
                "greylist_local entry deleted"
            );
            (StatusCode::NO_CONTENT, ()).into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("id {id} not found") })),
        )
            .into_response(),
        Err(e) => err500(format!("delete failed: {e}")).into_response(),
    }
}

// Suppress unused — these are kept for future error mapping clarity / test fixtures.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_helpers_shape() {
        let (code, _) = err400("bad");
        assert_eq!(code, StatusCode::BAD_REQUEST);
        let (code, _) = err403();
        assert_eq!(code, StatusCode::FORBIDDEN);
        let (code, _) = err409("dup");
        assert_eq!(code, StatusCode::CONFLICT);
    }

    #[test]
    fn create_request_deserializes_with_optional_note() {
        let req: CreateRequest = serde_json::from_str(
            r#"{"kind":"domain","list":"white","value":"example.com"}"#,
        )
        .unwrap();
        assert_eq!(req.kind, "domain");
        assert!(req.note.is_none());
    }


}
