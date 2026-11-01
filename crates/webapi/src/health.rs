//! `/api/health`, `/api/readiness`, `/api/status`.

use std::sync::Arc;

use crate::{WebState, handlers};

/// /api/health — public liveness probe. No auth required. Shape is:
///
/// ```json
/// {
///   "status": "healthy",
///   "ok": true,
///   "service": "mailrs-webapi",
///   "version": "<pkg-version>",
///   "uptime_secs": 42,
///   "kevy": true,
///   "pg": null
/// }
/// ```
///
/// Callers can rely on:
///   - the four `service` / `version` / `uptime_secs` / `status` fields
///     always being present.
///   - `kevy` reporting the real ping status (this handler round-trips
///     a kevy op to compute the boolean, so it's not just a config
///     flag).
///   - `pg` being `null` in the fastcore lane (no PostgreSQL backend
///     exists), so the frontend can hide the PG pill instead of
///     drawing it as "down". In the spg-backed lane the same handler
///     ships with `pg` set to a real probe.
pub(crate) async fn health_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    let uptime_secs = state.started_at.elapsed().as_secs();
    // Cheap kevy round-trip. Any success => backend healthy; any error
    // => backend unreachable. Runs on the shared shard connection, no
    // fresh TCP per request.
    let kevy_ok =
        handlers::kevy_util::with_kevy(|c| c.ping().map_err(std::io::Error::from)).is_ok();
    let screening = handlers::screening_admin::health();
    // the last daily check of the audit hash chain; null before the first
    let audit_chain =
        handlers::kevy_util::with_kevy(mailrs_core_sidestate::families::audit_chain::read_status)
            .ok()
            .flatten();
    let core_memory = core_memory(&state).await;
    // the daily check of this server's sending addresses against blocklists
    let egress =
        handlers::kevy_util::with_kevy(mailrs_core_sidestate::families::egress::read_zone_check)
            .map(|check| {
                mailrs_core_sidestate::families::egress::health(
                    check.as_ref(),
                    chrono::Utc::now().timestamp(),
                )
            })
            .unwrap_or(serde_json::Value::Null);
    axum::Json(serde_json::json!({
        "status": if kevy_ok { "healthy" } else { "degraded" },
        "ok": kevy_ok,
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime_secs,
        "kevy": kevy_ok,
        "pg": serde_json::Value::Null,
        "reputation": screening["reputation"],
        "attachment_scan": screening["attachment_scan"],
        "screening_rules": screening["screening_rules"],
        "audit_chain": audit_chain,
        "core_memory": core_memory,
        "egress": egress,
        "receiver": handlers::smtp_monitor::health(&state.receiver_stats),
        "jmap_uploads": handlers::jmap::upload_health(),
    }))
}

/// The core's container memory, and whether it is at or past
/// `MAILRS_MEMORY_HIGH_PERCENT` (80 by default) of its limit, for a watch
/// that compares one value; null when the core does not answer within two
/// seconds or cannot read its cgroup, and `high` null without a limit.
async fn core_memory(state: &WebState) -> serde_json::Value {
    let answer =
        tokio::time::timeout(std::time::Duration::from_secs(2), state.core.healthz()).await;
    let Ok(Ok(health)) = answer else {
        return serde_json::Value::Null;
    };
    let Some(m) = health.memory else {
        return serde_json::Value::Null;
    };
    let percent: u64 = std::env::var("MAILRS_MEMORY_HIGH_PERCENT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(80);
    serde_json::json!({
        "used_bytes": m.used_bytes,
        "limit_bytes": m.limit_bytes,
        "kevy_used_bytes": m.kevy_used_bytes,
        "kevy_keys": m.kevy_keys,
        "high": m.limit_bytes.map(|limit| is_high(m.used_bytes, limit, percent)),
    })
}

/// At or past `percent` of `limit`.
fn is_high(used: u64, limit: u64, percent: u64) -> bool {
    u128::from(used) * 100 >= u128::from(limit) * u128::from(percent)
}

/// /api/readiness — deep probe: does core RPC answer?
pub(crate) async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> Result<axum::Json<serde_json::Value>, axum::http::StatusCode> {
    match state.core.readyz().await {
        Ok(h) if h.ready => Ok(axum::Json(serde_json::json!({
            "status": "ready",
            "core_version": h.version,
        }))),
        Ok(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
        Err(_) => Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// /api/status — version + build info + webapi lifetime. No auth
/// required.
///
/// The SMTP counters live in `mailrs-receiver`, a different process in
/// the four-process split. They reach here over the monitor's stats
/// channel (`handlers::smtp_monitor`) and are served from the cache that
/// subscriber fills. Until a frame arrives they stay `null`, which UIs
/// render as a dash — v1.9.4 shipped a monitor page that read the absent
/// fields as `0` and displayed `NaN` uptime, and "unknown" told as
/// "zero" is the failure this endpoint exists to avoid. `queue` is still
/// null: its counter is in `mailrs-fastcore-sender` and nothing
/// publishes it yet.
pub(crate) async fn status_handler(
    axum::extract::State(state): axum::extract::State<Arc<WebState>>,
) -> axum::Json<serde_json::Value> {
    // The three SMTP counters belong to `mailrs-receiver`, a different
    // process. They arrive over the monitor's stats channel and are
    // cached here; `None` until one does, which is not the same as zero.
    let stats = state.receiver_stats.get();
    axum::Json(serde_json::json!({
        "service": "mailrs-webapi",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": state.started_at.elapsed().as_secs(),
        "active_connections": stats.map(|s| s.active_connections),
        "total_connections": stats.map(|s| s.total_connections),
        "total_messages": stats.map(|s| s.total_messages),
        "receiver_uptime_secs": stats.map(|s| s.uptime_secs),
        "queue": serde_json::Value::Null,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn high_is_at_or_past_the_percentage() {
        let gib = 1u64 << 30;
        assert!(!super::is_high(3 * gib, 4 * gib, 80));
        assert!(super::is_high(8 * gib, 10 * gib, 80));
        assert!(!super::is_high(8 * gib - 1, 10 * gib, 80));
        assert!(super::is_high(4 * gib, 4 * gib, 80));
        assert!(!super::is_high(2 * gib, 4 * gib, 90));
    }
}
