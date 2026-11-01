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
    let kevy_server_memory = kevy_server_memory();
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
        "kevy_server_memory": kevy_server_memory,
        "egress": egress,
        "receiver": handlers::smtp_monitor::health(&state.receiver_stats),
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
    let percent = high_percent();
    serde_json::json!({
        "used_bytes": m.used_bytes,
        "limit_bytes": m.limit_bytes,
        "kevy_used_bytes": m.kevy_used_bytes,
        "kevy_keys": m.kevy_keys,
        "high": m.limit_bytes.map(|limit| is_high(m.used_bytes, limit, percent)),
    })
}

/// kevy-server's memory: its resident size and its own count as `INFO
/// memory` reports them, the container limit the compose file hands this
/// process as `MAILRS_KEVY_SERVER_MEMORY_LIMIT` (the same value as that
/// service's `mem_limit`, e.g. `1g`), and whether the resident size is at
/// or past `MAILRS_MEMORY_HIGH_PERCENT` of it. Null when kevy-server does
/// not answer; `limit_bytes` and `high` null without a readable limit.
fn kevy_server_memory() -> serde_json::Value {
    use kevy_client::Reply;
    let info = handlers::kevy_util::with_kevy(|c| {
        c.pipeline(|p| {
            p.cmd(&[b"INFO", b"memory"]);
        })
        .map_err(std::io::Error::from)
    });
    let text = match info.as_deref() {
        Ok([Reply::Bulk(b)]) => String::from_utf8_lossy(b).into_owned(),
        _ => return serde_json::Value::Null,
    };
    let field = |name: &str| info_field(&text, name);
    let Some(used) = field("process_rss_bytes") else {
        return serde_json::Value::Null;
    };
    let limit = std::env::var("MAILRS_KEVY_SERVER_MEMORY_LIMIT")
        .ok()
        .and_then(|v| parse_size(&v));
    let percent = high_percent();
    serde_json::json!({
        "used_bytes": used,
        "limit_bytes": limit,
        "kevy_used_bytes": field("used_memory"),
        "high": limit.map(|l| is_high(used, l, percent)),
    })
}

/// The percentage of a limit at which memory reads as high.
fn high_percent() -> u64 {
    std::env::var("MAILRS_MEMORY_HIGH_PERCENT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(80)
}

/// One `name:value` line of an `INFO` reply, as a number.
fn info_field(info: &str, name: &str) -> Option<u64> {
    info.lines()
        .filter_map(|l| l.trim_end().split_once(':'))
        .find(|(k, _)| *k == name)
        .and_then(|(_, v)| v.trim().parse().ok())
}

/// A compose-style size: bytes, or a number with `k`, `m` or `g` (binary).
fn parse_size(v: &str) -> Option<u64> {
    let v = v.trim().to_ascii_lowercase();
    let v = v.strip_suffix('b').unwrap_or(&v);
    let (num, shift) = match v.as_bytes().last()? {
        b'k' => (&v[..v.len() - 1], 10),
        b'm' => (&v[..v.len() - 1], 20),
        b'g' => (&v[..v.len() - 1], 30),
        _ => (v, 0),
    };
    num.parse::<u64>().ok()?.checked_mul(1 << shift)
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
    fn a_compose_size_reads_as_bytes() {
        assert_eq!(super::parse_size("1g"), Some(1 << 30));
        assert_eq!(super::parse_size("512m"), Some(512 << 20));
        assert_eq!(super::parse_size("256K"), Some(256 << 10));
        assert_eq!(super::parse_size("1048576"), Some(1 << 20));
        assert_eq!(super::parse_size("1gb"), Some(1 << 30));
        assert_eq!(super::parse_size(""), None);
        assert_eq!(super::parse_size("lots"), None);
    }

    #[test]
    fn an_info_field_is_read_by_its_exact_name() {
        let info = "# Memory\r\nused_memory:15367747\r\nused_memory_peak:20000000\r\n\
                    process_rss_bytes:47800320\r\n";
        assert_eq!(super::info_field(info, "used_memory"), Some(15_367_747));
        assert_eq!(
            super::info_field(info, "process_rss_bytes"),
            Some(47_800_320)
        );
        assert_eq!(super::info_field(info, "maxmemory"), None);
    }

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
