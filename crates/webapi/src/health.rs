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
    let core_health = core_health(&state).await;
    let core_memory = core_memory(core_health.as_ref());
    let (spool, set_aside) = loop_health(core_health.as_ref());
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
        "new_domains": new_domains(),
        "quota": quota(&state).await,
        "fraud_hits": fraud_hits(),
        "receiver": handlers::smtp_monitor::health(&state.receiver_stats),
        "jmap_uploads": handlers::upload_sweep::health(),
        "spool": spool,
        "set_aside": set_aside,
    }))
}

/// How many domains have a first-seen time (`mailrs:domfirst:`), and how
/// many were first seen in the last 24 hours, for a watch on how fast the
/// set grows. Counted by a scan, so at most once every five minutes; null
/// when the shared store cannot be read.
fn new_domains() -> serde_json::Value {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(Instant, serde_json::Value)>> = Mutex::new(None);
    let now = chrono::Utc::now().timestamp();
    let cached = LAST
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, v)| v);
    let value = match cached {
        Some(v) => v,
        None => {
            let census = handlers::kevy_util::with_kevy(move |c| {
                mailrs_core_sidestate::families::domain_first::census(c, now)
            });
            let Ok(c) = census else {
                return serde_json::Value::Null;
            };
            let v = serde_json::json!({
                "keys": c.keys,
                "added_24h": c.added_24h,
                "computed_at": now,
            });
            if let Ok(mut last) = LAST.lock() {
                *last = Some((Instant::now(), v.clone()));
            }
            v
        }
    };
    with_staleness(value, now)
}

/// Hits per enforced rule of the active fraud bundle, today (UTC) and
/// over the last 30 days, from the receiver's daily counters. Every
/// enforced rule is listed, at 0 when it never fired, so a watch on one
/// rule has a number to compare. Beside them, `report-no-fingerprint`
/// counts scam reports whose message was too short to compare with others.
/// Recounted at most once every five minutes; null when the shared store
/// cannot be read.
fn fraud_hits() -> serde_json::Value {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(Instant, serde_json::Value)>> = Mutex::new(None);
    let now = chrono::Utc::now().timestamp();
    let cached = LAST
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, v)| v);
    if let Some(v) = cached {
        return with_staleness(v, now);
    }
    let (version, mut rules) = mailrs_fraud_lua::active_rule_set();
    // not a rule: reports that could not look for other copies
    rules.push(mailrs_core_sidestate::families::reports::NO_FINGERPRINT.to_string());
    let counts = handlers::kevy_util::with_kevy(move |c| {
        mailrs_core_sidestate::families::fraud_hits::counts(c, &rules, now)
    });
    let Ok(counts) = counts else {
        return serde_json::Value::Null;
    };
    let v = fraud_hits_value(&version, &counts, now);
    if let Ok(mut last) = LAST.lock() {
        *last = Some((Instant::now(), v.clone()));
    }
    with_staleness(v, now)
}

fn fraud_hits_value(
    version: &str,
    c: &mailrs_core_sidestate::families::fraud_hits::Counts,
    now: i64,
) -> serde_json::Value {
    serde_json::json!({
        "version": version,
        "by_rule_today": c.today,
        "by_rule_30d": c.last_30d,
        "computed_at": now,
    })
}

/// How full the accounts are, from each account's usage counter against
/// its limit; no maildir is read. `over_80` and `max_used_ratio` count only
/// accounts with a limit, `accounts_without_limit` the rest, so a watch can
/// also see limits missing. Recounted at most once every five minutes;
/// null when the core or the shared store cannot be read.
async fn quota(state: &WebState) -> serde_json::Value {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<(Instant, serde_json::Value)>> = Mutex::new(None);
    let now = chrono::Utc::now().timestamp();
    let cached = LAST
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, v)| v);
    if let Some(v) = cached {
        return with_staleness(v, now);
    }
    let listed = tokio::time::timeout(Duration::from_secs(2), state.core.list_accounts()).await;
    let Ok(Ok(list)) = listed else {
        return serde_json::Value::Null;
    };
    let users: Vec<String> = list.items.into_iter().map(|a| a.address).collect();
    let census = handlers::kevy_util::with_kevy(move |c| {
        mailrs_core_sidestate::families::quota_limits::census(c, &users)
    });
    let Ok(census) = census else {
        return serde_json::Value::Null;
    };
    let v = quota_value(&census, now);
    if let Ok(mut last) = LAST.lock() {
        *last = Some((Instant::now(), v.clone()));
    }
    with_staleness(v, now)
}

fn quota_value(
    c: &mailrs_core_sidestate::families::quota_limits::Census,
    now: i64,
) -> serde_json::Value {
    serde_json::json!({
        "accounts": c.accounts,
        "accounts_without_limit": c.accounts_without_limit,
        "over_80": c.over_80,
        "max_used_ratio": c.max_used_ratio,
        "computed_at": now,
    })
}

/// `stale` is true once the counts are more than 15 minutes old, for a
/// watch that can only compare a value with a constant.
fn with_staleness(mut value: serde_json::Value, now: i64) -> serde_json::Value {
    let computed_at = value["computed_at"].as_i64().unwrap_or(0);
    if let Some(o) = value.as_object_mut() {
        o.insert("stale".into(), serde_json::json!(now - computed_at > 900));
    }
    value
}

/// The core's own health answer; `None` when it does not answer within two
/// seconds.
async fn core_health(state: &WebState) -> Option<mailrs_core_api::types::HealthResponse> {
    let answer =
        tokio::time::timeout(std::time::Duration::from_secs(2), state.core.healthz()).await;
    answer.ok().and_then(Result::ok)
}

/// The spool the core drains and what its loops gave up on:
///
/// - `spool`: `{quarantined, oldest_incoming_age_seconds}` — spool files
///   set aside because the core died on each three times, and the age of
///   the oldest file waiting to be delivered (0 when none is);
/// - `set_aside`: `{attachment_text, fraud_backfill, sender}` — attachment
///   queue items and fraud-backfill conversations the core gave up on, and
///   outbound jobs the sender set aside after dying on them.
///
/// `spool` is null when the core does not answer; inside `set_aside`, each
/// count is null when its store cannot be read or (`fraud_backfill`) no
/// backfill runs.
fn loop_health(
    health: Option<&mailrs_core_api::types::HealthResponse>,
) -> (serde_json::Value, serde_json::Value) {
    let spool = health
        .and_then(|h| h.spool)
        .map_or(serde_json::Value::Null, |s| {
            serde_json::json!({
                "quarantined": s.quarantined,
                "oldest_incoming_age_seconds": s.oldest_incoming_age_seconds,
            })
        });
    let core = health.and_then(|h| h.set_aside);
    let sender = handlers::kevy_util::with_kevy(|c| {
        c.zcard(mailrs_core_sidestate::families::outbound::SET_ASIDE_IDX)
            .map_err(std::io::Error::from)
    })
    .ok();
    let set_aside = serde_json::json!({
        "attachment_text": core.and_then(|s| s.attachment_text),
        "fraud_backfill": core.and_then(|s| s.fraud_backfill),
        "sender": sender,
    });
    (spool, set_aside)
}

/// The core's container memory, and whether it is at or past
/// `MAILRS_MEMORY_HIGH_PERCENT` (80 by default) of its limit, for a watch
/// that compares one value; null when the core does not answer within two
/// seconds or cannot read its cgroup, and `high` null without a limit.
fn core_memory(health: Option<&mailrs_core_api::types::HealthResponse>) -> serde_json::Value {
    let Some(m) = health.and_then(|h| h.memory) else {
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
    let stats = stats.as_ref();
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
    fn every_enforced_rule_is_listed_and_one_that_never_fired_reads_zero() {
        let mut c = mailrs_core_sidestate::families::fraud_hits::Counts::default();
        c.today.insert("svg-runs-script".into(), 0);
        c.today.insert("html-builds-a-download".into(), 2);
        c.last_30d.insert("svg-runs-script".into(), 0);
        c.last_30d.insert("html-builds-a-download".into(), 5);
        let v = super::fraud_hits_value("lua:abc", &c, 7);
        assert_eq!(v["version"], "lua:abc");
        assert_eq!(v["by_rule_today"]["svg-runs-script"], 0);
        assert_eq!(v["by_rule_today"]["html-builds-a-download"], 2);
        assert_eq!(v["by_rule_30d"]["html-builds-a-download"], 5);
        assert_eq!(v["computed_at"], 7);
        assert_eq!(super::with_staleness(v, 7)["stale"], false);
    }

    #[test]
    fn no_account_with_a_limit_reads_as_no_ratio_not_zero() {
        let census = mailrs_core_sidestate::families::quota_limits::Census {
            accounts: 2,
            accounts_without_limit: 2,
            over_80: 0,
            max_used_ratio: None,
        };
        let v = super::quota_value(&census, 7);
        assert!(v["max_used_ratio"].is_null(), "{v}");
        assert_eq!(v["accounts_without_limit"], 2);
        assert_eq!(v["computed_at"], 7);
    }

    #[test]
    fn new_domain_counts_older_than_fifteen_minutes_are_stale() {
        let v = serde_json::json!({ "keys": 1, "added_24h": 0, "computed_at": 1_000 });
        assert_eq!(super::with_staleness(v.clone(), 1_900)["stale"], false);
        assert_eq!(super::with_staleness(v, 1_901)["stale"], true);
    }

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
