use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use super::*;

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

pub(crate) async fn prometheus_metrics(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    use std::fmt::Write;
    use std::sync::atomic::Ordering;

    let uptime = state.started_at.elapsed().as_secs();
    let total_connections = state.total_connections.load(Ordering::Relaxed);
    let active_connections = state.active_connections.load(Ordering::Relaxed);
    let total_messages = state.total_messages.load(Ordering::Relaxed);
    let active_sessions = state.sessions.len() as u64;
    let inbound_accept = state.inbound_accept_total.load(Ordering::Relaxed);
    let inbound_reject = state.inbound_reject_total.load(Ordering::Relaxed);
    let inbound_defer = state.inbound_defer_total.load(Ordering::Relaxed);
    let inbound_junk = state.inbound_junk_total.load(Ordering::Relaxed);
    let account_cache_size = state
        .domain_store
        .as_ref()
        .map(|ds| ds.cache_size())
        .unwrap_or(0) as u64;

    let (pg_up, valkey_up) = match &state.health {
        Some(h) => (h.pg_up(), h.valkey_up()),
        None => (false, false),
    };

    let (pending, delivered, failed, bounced) = if let Some(ref pool) = state.outbound_queue {
        match mailrs_outbound_queue::queue::queue_stats(pool).await {
            Ok(stats) => {
                let mut p = 0i64;
                let mut d = 0i64;
                let mut f = 0i64;
                let mut b = 0i64;
                for (status, count) in stats {
                    match status.as_str() {
                        "pending" | "inflight" => p += count,
                        "delivered" => d = count,
                        "failed" => f = count,
                        "bounced" => b = count,
                        _ => {}
                    }
                }
                (p, d, f, b)
            }
            Err(_) => (0, 0, 0, 0),
        }
    } else {
        (0, 0, 0, 0)
    };

    let mut body = String::with_capacity(1024);
    let _ = writeln!(body, "# HELP mailrs_uptime_seconds Time since server start");
    let _ = writeln!(body, "# TYPE mailrs_uptime_seconds gauge");
    let _ = writeln!(body, "mailrs_uptime_seconds {uptime}");
    let _ = writeln!(body, "# HELP mailrs_connections_total Total connections accepted");
    let _ = writeln!(body, "# TYPE mailrs_connections_total counter");
    let _ = writeln!(body, "mailrs_connections_total {total_connections}");
    let _ = writeln!(body, "# HELP mailrs_connections_active Currently open connections");
    let _ = writeln!(body, "# TYPE mailrs_connections_active gauge");
    let _ = writeln!(body, "mailrs_connections_active {active_connections}");
    let _ = writeln!(body, "# HELP mailrs_messages_total Total messages delivered locally");
    let _ = writeln!(body, "# TYPE mailrs_messages_total counter");
    let _ = writeln!(body, "mailrs_messages_total {total_messages}");
    let _ = writeln!(body, "# HELP mailrs_active_sessions Active web sessions");
    let _ = writeln!(body, "# TYPE mailrs_active_sessions gauge");
    let _ = writeln!(body, "mailrs_active_sessions {active_sessions}");
    let _ = writeln!(body, "# HELP mailrs_account_cache_size Domain store cache entries");
    let _ = writeln!(body, "# TYPE mailrs_account_cache_size gauge");
    let _ = writeln!(body, "mailrs_account_cache_size {account_cache_size}");
    let _ = writeln!(body, "# HELP mailrs_inbound_verdict_total Inbound DATA decisions by verdict (since process start)");
    let _ = writeln!(body, "# TYPE mailrs_inbound_verdict_total counter");
    let _ = writeln!(body, "mailrs_inbound_verdict_total{{verdict=\"accept\"}} {inbound_accept}");
    let _ = writeln!(body, "mailrs_inbound_verdict_total{{verdict=\"reject\"}} {inbound_reject}");
    let _ = writeln!(body, "mailrs_inbound_verdict_total{{verdict=\"defer\"}} {inbound_defer}");
    let _ = writeln!(body, "mailrs_inbound_verdict_total{{verdict=\"junk\"}} {inbound_junk}");
    let _ = writeln!(body, "# HELP mailrs_queue_pending Pending outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_pending gauge");
    let _ = writeln!(body, "mailrs_queue_pending {pending}");
    let _ = writeln!(body, "# HELP mailrs_queue_delivered Delivered outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_delivered gauge");
    let _ = writeln!(body, "mailrs_queue_delivered {delivered}");
    let _ = writeln!(body, "# HELP mailrs_queue_failed Failed outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_failed gauge");
    let _ = writeln!(body, "mailrs_queue_failed {failed}");
    let _ = writeln!(body, "# HELP mailrs_queue_bounced Bounced outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_bounced gauge");
    let _ = writeln!(body, "mailrs_queue_bounced {bounced}");
    let _ = writeln!(body, "# HELP mailrs_health_pg_up PostgreSQL availability");
    let _ = writeln!(body, "# TYPE mailrs_health_pg_up gauge");
    let _ = writeln!(body, "mailrs_health_pg_up {}", if pg_up { 1 } else { 0 });
    let _ = writeln!(body, "# HELP mailrs_health_valkey_up Valkey/Redis availability");
    let _ = writeln!(body, "# TYPE mailrs_health_valkey_up gauge");
    let _ = writeln!(body, "mailrs_health_valkey_up {}", if valkey_up { 1 } else { 0 });

    // suppression list count
    if let Some(ref pool) = state.pg_pool {
        let suppression_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM suppression_list")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        let _ = writeln!(body, "# HELP mailrs_suppression_count Suppressed email addresses");
        let _ = writeln!(body, "# TYPE mailrs_suppression_count gauge");
        let _ = writeln!(body, "mailrs_suppression_count {suppression_count}");
    }

    // RBL listing status
    if let Some(ref valkey) = state.valkey {
        let rbl_listed: i64 = {
            let keys: Vec<String> = redis::cmd("KEYS").arg("rbl:status:*")
                .query_async(&mut valkey.clone()).await.unwrap_or_default();
            let mut listed = 0i64;
            for key in &keys {
                if let Ok(json) = redis::cmd("GET").arg(key)
                    .query_async::<String>(&mut valkey.clone()).await
                    && json.contains("\"any_listed\":true") { listed += 1; }
            }
            listed
        };
        let _ = writeln!(body, "# HELP mailrs_rbl_listed IPs currently listed on RBLs");
        let _ = writeln!(body, "# TYPE mailrs_rbl_listed gauge");
        let _ = writeln!(body, "mailrs_rbl_listed {rbl_listed}");
    }

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
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
    let (status_label, level, pg, valkey, uptime) = match &state.health {
        Some(h) => (h.status_label(), h.level(), h.pg_up(), h.valkey_up(), h.uptime_secs()),
        None => ("unhealthy", 3, false, false, state.started_at.elapsed().as_secs()),
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": status_label,
            "level": level,
            "pg": pg,
            "valkey": valkey,
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
    let ready = state
        .health
        .as_ref()
        .map(|h| h.is_ready())
        .unwrap_or(false);
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(serde_json::json!({ "ready": ready })))
}
