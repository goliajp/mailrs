//! Webhook subscription admin — direct SQL through `state.pool`.
//! Existing `crates/server/src/webhook/store.rs` is not modified (ironrule).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rand_core::{OsRng, RngCore};

use mailrs_core_api::method::admin as wire;

use crate::core_rpc::CoreRpcState;

/// 64-hex-char (32-byte) signing secret, same shape as webhook::store.
fn generate_signing_secret() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// POST /v1/admin/webhook-subscriptions
pub async fn create_webhook(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::CreateWebhookRequest>,
) -> Result<Json<wire::CreateWebhookResponse>, StatusCode> {
    if req.url.is_empty() || req.event_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let secret = generate_signing_secret();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO webhook_subscriptions \
             (account_address, url, event_type, filter_sender, filter_thread_id, signing_secret) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(&req.account_address)
    .bind(&req.url)
    .bind(&req.event_type)
    .bind(req.filter_sender.as_deref())
    .bind(req.filter_thread_id.as_deref())
    .bind(&secret)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, account = %req.account_address, "create_webhook failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(wire::CreateWebhookResponse {
        id,
        signing_secret: secret,
    }))
}

/// GET /v1/admin/accounts/{address}/webhook-subscriptions
pub async fn list_webhooks(
    State(state): State<Arc<CoreRpcState>>,
    Path(address): Path<String>,
) -> Result<Json<wire::WebhookListResponse>, StatusCode> {
    #[allow(clippy::type_complexity)]
    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            bool,
            i64,
        ),
    >(
        "SELECT id, account_address, url, event_type, filter_sender, filter_thread_id, \
                signing_secret, active, \
                EXTRACT(EPOCH FROM created_at)::bigint \
         FROM webhook_subscriptions WHERE account_address = $1 AND active = true \
         ORDER BY created_at DESC",
    )
    .bind(&address)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, address = %address, "list_webhooks failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let items = rows
        .into_iter()
        .map(
            |(
                id,
                account_address,
                url,
                event_type,
                filter_sender,
                filter_thread_id,
                signing_secret,
                active,
                created_at,
            )| wire::WebhookSubWire {
                id,
                account_address,
                url,
                event_type,
                filter_sender,
                filter_thread_id,
                signing_secret,
                active,
                created_at,
            },
        )
        .collect();
    Ok(Json(wire::WebhookListResponse { items }))
}

/// DELETE /v1/admin/webhook-subscriptions/{id}  — soft delete (active=false).
pub async fn delete_webhook(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let result = sqlx::query(
        "UPDATE webhook_subscriptions SET active = false WHERE id = $1 AND active = true",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, id, "delete_webhook failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if result.rows_affected() == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
