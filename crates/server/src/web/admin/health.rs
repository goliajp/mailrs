use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::*;

mod metrics;
pub(crate) use metrics::prometheus_metrics;

// ---------- status + queue endpoints ----------

#[derive(serde::Serialize)]
pub(crate) struct StatusResponse {
    pub uptime_secs: u64,
    pub active_connections: u64,
    pub total_connections: u64,
    pub total_messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueStatsResp>,
}

#[derive(serde::Serialize)]
pub(crate) struct QueueStatsResp {
    pub pending: i64,
    pub inflight: i64,
    pub delivered: i64,
    pub failed: i64,
    pub bounced: i64,
}

pub(crate) async fn get_status(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    use std::sync::atomic::Ordering;

    let queue = if let Some(ref pool) = state.outbound_queue {
        match mailrs_outbound_queue::queue::queue_stats(pool).await {
            Ok(stats) => {
                let mut qs = QueueStatsResp {
                    pending: 0,
                    inflight: 0,
                    delivered: 0,
                    failed: 0,
                    bounced: 0,
                };
                for (status, count) in stats {
                    match status.as_str() {
                        "pending" => qs.pending = count,
                        "inflight" => qs.inflight = count,
                        "delivered" => qs.delivered = count,
                        "failed" => qs.failed = count,
                        "bounced" => qs.bounced = count,
                        _ => {}
                    }
                }
                Some(qs)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    Json(StatusResponse {
        uptime_secs: state.started_at.elapsed().as_secs(),
        active_connections: state.active_connections.load(Ordering::Relaxed),
        total_connections: state.total_connections.load(Ordering::Relaxed),
        total_messages: state.total_messages.load(Ordering::Relaxed),
        queue,
    })
}

pub(crate) async fn get_health(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let (status_label, level, pg, kevy, uptime) = match &state.health {
        Some(h) => (
            h.status_label(),
            h.level(),
            h.pg_up(),
            h.kevy_up(),
            h.uptime_secs(),
        ),
        None => (
            "unhealthy",
            3,
            false,
            false,
            state.started_at.elapsed().as_secs(),
        ),
    };
    // One Store::info() call per /api/health hit. Docker healthcheck
    // polls this every few seconds, but info() is a single-mutex
    // snapshot (~µs), so the overhead is negligible vs the value of a
    // single curl giving you keys / aof_bytes / expire_pending at a
    // glance — without scraping prometheus.
    let kevy_info = state.kevy_embed.as_ref().map(|s| {
        let i = s.info();
        serde_json::json!({
            "keys": i.keys,
            "used_memory_bytes": i.used_memory,
            "aof_bytes": i.aof_bytes,
            "expire_pending": i.expire_pending,
            "evictions": i.evictions,
            "expired_keys": i.expired_keys,
        })
    });
    // Phase 2 greylist local lists snapshot — read-only summary for
    // operator visibility (the same data the admin UI's status pill
    // renders). The RwLock acquire is `.read()`, contention-free under
    // normal load.
    let greylist_local = {
        let g = state.greylist_local.read().await;
        serde_json::json!({
            "white": g.white_count(),
            "black": g.black_count(),
            "last_reload_at": g.last_reload_at,
            "last_error": g.last_error,
        })
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": status_label,
            "level": level,
            "pg": pg,
            "kevy": kevy,
            "kevy_info": kevy_info,
            "greylist_local": greylist_local,
            "uptime_secs": uptime,
            "version": env!("CARGO_PKG_VERSION"),
            "active_sessions": state.sessions.len(),
            "account_cache_size": state.domain_store.as_ref().map(|ds| ds.cache_size()).unwrap_or(0),
            "total_connections": state.total_connections.load(std::sync::atomic::Ordering::Relaxed),
            "total_messages": state.total_messages.load(std::sync::atomic::Ordering::Relaxed),
        })),
    )
}

/// kubernetes-style readiness probe: returns 200 if PG is up, 503 otherwise
pub(crate) async fn get_readiness(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let ready = state.health.as_ref().map(|h| h.is_ready()).unwrap_or(false);
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(serde_json::json!({ "ready": ready })))
}
