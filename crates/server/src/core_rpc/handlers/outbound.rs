//! Handlers for `mailrs_core_api::method::outbound` — sender ↔ core RPC.
//!
//! Phase 4.4 — minimal claim / mark_delivered / mark_failed / mark_bounced /
//! stats. Uses the existing `mailrs_outbound_queue` crate's free
//! functions; never touches outbound-queue source (ironrule).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;

use mailrs_core_api::method::outbound as wire;
use mailrs_outbound_queue::queue;

use crate::core_rpc::CoreRpcState;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn map_queue_status(s: queue::QueueStatus) -> wire::QueueStatus {
    match s {
        queue::QueueStatus::Pending => wire::QueueStatus::Pending,
        queue::QueueStatus::InFlight => wire::QueueStatus::Inflight,
        queue::QueueStatus::Delivered => wire::QueueStatus::Delivered,
        queue::QueueStatus::Failed => wire::QueueStatus::Failed,
        queue::QueueStatus::Bounced => wire::QueueStatus::Bounced,
    }
}

fn to_wire(m: queue::QueuedMessage) -> wire::OutboundMessageWire {
    wire::OutboundMessageWire {
        id: m.id,
        sender: m.sender,
        recipient: m.recipient,
        original_sender: None,
        message_data_base64: B64.encode(&m.message_data),
        status: map_queue_status(m.status),
        attempts: m.attempts,
        last_error: m.last_error,
        next_retry: Some(m.next_retry),
        scheduled_at: None,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

/// POST /v1/outbound/claim
pub async fn claim(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::ClaimRequest>,
) -> Result<Json<wire::ClaimResponse>, StatusCode> {
    let items = queue::claim_for_delivery(&state.pool, now_secs(), req.batch_size)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "outbound claim failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::ClaimResponse {
        items: items.into_iter().map(to_wire).collect(),
    }))
}

/// GET /v1/outbound/stats
pub async fn stats(
    State(state): State<Arc<CoreRpcState>>,
) -> Result<Json<wire::QueueStatsResponse>, StatusCode> {
    let rows = queue::queue_stats(&state.pool).await.map_err(|e| {
        tracing::warn!(error = %e, "outbound stats failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let mut resp = wire::QueueStatsResponse::default();
    for (status, n) in rows {
        match status.as_str() {
            "pending" => resp.pending = n,
            "inflight" => resp.inflight = n,
            "delivered" => resp.delivered = n,
            "failed" => resp.failed = n,
            "bounced" => resp.bounced = n,
            _ => {}
        }
    }
    Ok(Json(resp))
}

/// POST /v1/outbound/recover-stale
pub async fn recover_stale(
    State(state): State<Arc<CoreRpcState>>,
    Json(req): Json<wire::RecoverStaleRequest>,
) -> Result<Json<wire::RecoverStaleResponse>, StatusCode> {
    let cutoff = now_secs().saturating_sub(req.older_than_secs as i64);
    let recovered = queue::recover_stale_inflight(&state.pool, cutoff)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "outbound recover_stale failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(wire::RecoverStaleResponse {
        recovered: recovered as u32,
    }))
}

/// POST /v1/outbound/{id}/delivered
pub async fn mark_delivered(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    queue::mark_delivered(&state.pool, id, now_secs())
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, "mark_delivered failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/outbound/{id}/failed
pub async fn mark_failed(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
    Json(req): Json<wire::MarkFailedRequest>,
) -> Result<StatusCode, StatusCode> {
    let now = now_secs();
    queue::mark_failed(
        &state.pool,
        id,
        &req.error,
        req.next_retry.unwrap_or(now + 300),
        now,
    )
    .await
    .map_err(|e| {
        tracing::warn!(error = %e, id, "mark_failed failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /v1/outbound/{id}/bounced
pub async fn mark_bounced(
    State(state): State<Arc<CoreRpcState>>,
    Path(id): Path<i64>,
    Json(req): Json<wire::MarkBouncedRequest>,
) -> Result<StatusCode, StatusCode> {
    queue::mark_bounced(&state.pool, id, &req.error, now_secs())
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, id, "mark_bounced failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}
