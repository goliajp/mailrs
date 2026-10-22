//! Webhook subscriptions — shared admin side-state served from the
//! network kevy, keyed exactly like webapi + pg-core:
//!   `admin:webhooks:{address}`      hash id → JSON WebhookSubWire
//!   `admin:webhooks:counter`        string next id

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{
    CreateWebhookRequest, CreateWebhookResponse, WebhookListResponse,
};

use crate::NetKevy;

pub async fn create_webhook<S: NetKevy>(
    State(state): State<Arc<S>>,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<Json<CreateWebhookResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let w = crate::families::webhooks::create(
        &mut conn,
        crate::families::webhooks::NewWebhook {
            account_address: req.account_address,
            url: req.url,
            event_type: req.event_type,
            filter_sender: req.filter_sender,
            filter_thread_id: req.filter_thread_id,
        },
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CreateWebhookResponse {
        id: w.id,
        signing_secret: w.signing_secret,
    }))
}

pub async fn list_webhooks<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(address): Path<String>,
) -> Json<WebhookListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(WebhookListResponse { items: Vec::new() });
    };
    let items = crate::families::webhooks::list(&mut conn, &address).unwrap_or_default();
    Json(WebhookListResponse { items })
}

pub async fn delete_webhook<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match crate::families::webhooks::delete(&mut conn, id) {
        Ok(true) => StatusCode::NO_CONTENT,
        // Nothing of that id. Previously this answered 204 either way, and
        // since it searched a swept index it always took this branch.
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
