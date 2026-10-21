//! The Prometheus scrape endpoint.

use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::response::IntoResponse;

use super::*;

pub(crate) async fn prometheus_metrics(State(state): State<Arc<WebState>>) -> impl IntoResponse {
    let c = Counters::read(&state);
    let (pending, delivered, failed, bounced) = outbound_queue_counts(&state).await;

    let mut body = String::with_capacity(1024);
    write_connection_gauges(&mut body, &c);
    write_verdict_counters(&mut body, &c);
    write_queue_and_health_gauges(&mut body, &c, (pending, delivered, failed, bounced));
    write_suppression_count(&mut body, &state).await;
    write_rbl_listed(&mut body, &state);
    write_kevy_info(&mut body, &state);
    append_metrics_facade(&mut body, &state);

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
}

/// The in-process counters and health flags, read together before the
/// queue query.
#[derive(Clone, Copy)]
struct Counters {
    uptime: u64,
    total_connections: u64,
    active_connections: u64,
    total_messages: u64,
    active_sessions: u64,
    inbound_accept: u64,
    inbound_reject: u64,
    inbound_defer: u64,
    inbound_junk: u64,
    auth_success: u64,
    auth_failure: u64,
    account_cache_size: u64,
    pg_up: bool,
    kevy_up: bool,
}

impl Counters {
    fn read(state: &WebState) -> Self {
        let uptime = state.started_at.elapsed().as_secs();
        let total_connections = state.total_connections.load(Ordering::Relaxed);
        let active_connections = state.active_connections.load(Ordering::Relaxed);
        let total_messages = state.total_messages.load(Ordering::Relaxed);
        let active_sessions = state.sessions.len() as u64;
        let inbound_accept = state.inbound_accept_total.load(Ordering::Relaxed);
        let inbound_reject = state.inbound_reject_total.load(Ordering::Relaxed);
        let inbound_defer = state.inbound_defer_total.load(Ordering::Relaxed);
        let inbound_junk = state.inbound_junk_total.load(Ordering::Relaxed);
        let auth_success = state.auth_success_total.load(Ordering::Relaxed);
        let auth_failure = state.auth_failure_total.load(Ordering::Relaxed);
        let account_cache_size = state
            .domain_store
            .as_ref()
            .map(|ds| ds.cache_size())
            .unwrap_or(0) as u64;
        let (pg_up, kevy_up) = match &state.health {
            Some(h) => (h.pg_up(), h.kevy_up()),
            None => (false, false),
        };
        Self {
            uptime,
            total_connections,
            active_connections,
            total_messages,
            active_sessions,
            inbound_accept,
            inbound_reject,
            inbound_defer,
            inbound_junk,
            auth_success,
            auth_failure,
            account_cache_size,
            pg_up,
            kevy_up,
        }
    }
}

async fn outbound_queue_counts(state: &WebState) -> (i64, i64, i64, i64) {
    if let Some(ref pool) = state.outbound_queue {
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
    }
}

fn write_connection_gauges(body: &mut String, c: &Counters) {
    let Counters {
        uptime,
        total_connections,
        active_connections,
        total_messages,
        active_sessions,
        account_cache_size,
        ..
    } = *c;
    let _ = writeln!(body, "# HELP mailrs_uptime_seconds Time since server start");
    let _ = writeln!(body, "# TYPE mailrs_uptime_seconds gauge");
    let _ = writeln!(body, "mailrs_uptime_seconds {uptime}");
    let _ = writeln!(
        body,
        "# HELP mailrs_connections_total Total connections accepted"
    );
    let _ = writeln!(body, "# TYPE mailrs_connections_total counter");
    let _ = writeln!(body, "mailrs_connections_total {total_connections}");
    let _ = writeln!(
        body,
        "# HELP mailrs_connections_active Currently open connections"
    );
    let _ = writeln!(body, "# TYPE mailrs_connections_active gauge");
    let _ = writeln!(body, "mailrs_connections_active {active_connections}");
    let _ = writeln!(
        body,
        "# HELP mailrs_messages_total Total messages delivered locally"
    );
    let _ = writeln!(body, "# TYPE mailrs_messages_total counter");
    let _ = writeln!(body, "mailrs_messages_total {total_messages}");
    let _ = writeln!(body, "# HELP mailrs_active_sessions Active web sessions");
    let _ = writeln!(body, "# TYPE mailrs_active_sessions gauge");
    let _ = writeln!(body, "mailrs_active_sessions {active_sessions}");
    let _ = writeln!(
        body,
        "# HELP mailrs_account_cache_size Domain store cache entries"
    );
    let _ = writeln!(body, "# TYPE mailrs_account_cache_size gauge");
    let _ = writeln!(body, "mailrs_account_cache_size {account_cache_size}");
}

fn write_verdict_counters(body: &mut String, c: &Counters) {
    let Counters {
        inbound_accept,
        inbound_reject,
        inbound_defer,
        inbound_junk,
        auth_success,
        auth_failure,
        ..
    } = *c;
    let _ = writeln!(
        body,
        "# HELP mailrs_inbound_verdict_total Inbound DATA decisions by verdict (since process start)"
    );
    let _ = writeln!(body, "# TYPE mailrs_inbound_verdict_total counter");
    let _ = writeln!(
        body,
        "mailrs_inbound_verdict_total{{verdict=\"accept\"}} {inbound_accept}"
    );
    let _ = writeln!(
        body,
        "mailrs_inbound_verdict_total{{verdict=\"reject\"}} {inbound_reject}"
    );
    let _ = writeln!(
        body,
        "mailrs_inbound_verdict_total{{verdict=\"defer\"}} {inbound_defer}"
    );
    let _ = writeln!(
        body,
        "mailrs_inbound_verdict_total{{verdict=\"junk\"}} {inbound_junk}"
    );
    let _ = writeln!(
        body,
        "# HELP mailrs_auth_total Web login attempts by outcome (since process start)"
    );
    let _ = writeln!(body, "# TYPE mailrs_auth_total counter");
    let _ = writeln!(
        body,
        "mailrs_auth_total{{outcome=\"success\"}} {auth_success}"
    );
    let _ = writeln!(
        body,
        "mailrs_auth_total{{outcome=\"failure\"}} {auth_failure}"
    );
}

fn write_queue_and_health_gauges(
    body: &mut String,
    c: &Counters,
    (pending, delivered, failed, bounced): (i64, i64, i64, i64),
) {
    let (pg_up, kevy_up) = (c.pg_up, c.kevy_up);
    let _ = writeln!(
        body,
        "# HELP mailrs_queue_pending Pending outbound messages"
    );
    let _ = writeln!(body, "# TYPE mailrs_queue_pending gauge");
    let _ = writeln!(body, "mailrs_queue_pending {pending}");
    let _ = writeln!(
        body,
        "# HELP mailrs_queue_delivered Delivered outbound messages"
    );
    let _ = writeln!(body, "# TYPE mailrs_queue_delivered gauge");
    let _ = writeln!(body, "mailrs_queue_delivered {delivered}");
    let _ = writeln!(body, "# HELP mailrs_queue_failed Failed outbound messages");
    let _ = writeln!(body, "# TYPE mailrs_queue_failed gauge");
    let _ = writeln!(body, "mailrs_queue_failed {failed}");
    let _ = writeln!(
        body,
        "# HELP mailrs_queue_bounced Bounced outbound messages"
    );
    let _ = writeln!(body, "# TYPE mailrs_queue_bounced gauge");
    let _ = writeln!(body, "mailrs_queue_bounced {bounced}");
    let _ = writeln!(body, "# HELP mailrs_health_pg_up PostgreSQL availability");
    let _ = writeln!(body, "# TYPE mailrs_health_pg_up gauge");
    let _ = writeln!(body, "mailrs_health_pg_up {}", if pg_up { 1 } else { 0 });
    let _ = writeln!(body, "# HELP mailrs_health_kevy_up Kevy/Redis availability");
    let _ = writeln!(body, "# TYPE mailrs_health_kevy_up gauge");
    let _ = writeln!(
        body,
        "mailrs_health_kevy_up {}",
        if kevy_up { 1 } else { 0 }
    );
}

async fn write_suppression_count(body: &mut String, state: &WebState) {
    // suppression list count
    if let Some(ref pool) = state.pg_pool {
        let suppression_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM suppression_list")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        let _ = writeln!(
            body,
            "# HELP mailrs_suppression_count Suppressed email addresses"
        );
        let _ = writeln!(body, "# TYPE mailrs_suppression_count gauge");
        let _ = writeln!(body, "mailrs_suppression_count {suppression_count}");
    }
}

fn write_rbl_listed(body: &mut String, state: &WebState) {
    // RBL listing status
    if let Some(ref store) = state.kevy_embed {
        // collect every rbl:status:* key, count those whose JSON value
        // has any_listed:true. kevy-store's collect_keys takes a glob-
        // style pattern matching Redis KEYS semantics.
        let keys: Vec<Vec<u8>> = store.collect_keys(Some(b"rbl:status:*"), None);
        let mut rbl_listed: i64 = 0;
        for key in &keys {
            if let Ok(Some(bytes)) = store.get(key)
                && let Ok(json) = String::from_utf8(bytes)
                && json.contains("\"any_listed\":true")
            {
                rbl_listed += 1;
            }
        }
        let _ = writeln!(
            body,
            "# HELP mailrs_rbl_listed IPs currently listed on RBLs"
        );
        let _ = writeln!(body, "# TYPE mailrs_rbl_listed gauge");
        let _ = writeln!(body, "mailrs_rbl_listed {rbl_listed}");
    }
}

fn write_kevy_info(body: &mut String, state: &WebState) {
    // Kevy current-state snapshot. One Store::info() call per scrape,
    // takes the kevy mutex once. Replay/Rewrite event metrics come
    // through the metric-sink path (see kevy_store::emit_kevy_metric);
    // these gauges are the complementary "live keyspace right now"
    // snapshot view.
    if let Some(ref store) = state.kevy_embed {
        let info = store.info();
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_keys Live key count (DBSIZE)\n# TYPE mailrs_kevy_keys gauge\nmailrs_kevy_keys {}",
            info.keys
        );
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_used_memory_bytes Estimated resident bytes\n# TYPE mailrs_kevy_used_memory_bytes gauge\nmailrs_kevy_used_memory_bytes {}",
            info.used_memory
        );
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_aof_bytes Current on-disk AOF size\n# TYPE mailrs_kevy_aof_bytes gauge\nmailrs_kevy_aof_bytes {}",
            info.aof_bytes
        );
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_expire_pending Live keys carrying a TTL (0 when TTL expected = TTL subsystem bug)\n# TYPE mailrs_kevy_expire_pending gauge\nmailrs_kevy_expire_pending {}",
            info.expire_pending
        );
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_evictions_total Cumulative keys evicted by maxmemory\n# TYPE mailrs_kevy_evictions_total counter\nmailrs_kevy_evictions_total {}",
            info.evictions
        );
        let _ = writeln!(
            body,
            "# HELP mailrs_kevy_expired_keys_total Cumulative keys expired (lazy + active reaper)\n# TYPE mailrs_kevy_expired_keys_total counter\nmailrs_kevy_expired_keys_total {}",
            info.expired_keys
        );
    }
}

fn append_metrics_facade(body: &mut String, state: &WebState) {
    // Append metrics-rs facade output (IMAP/POP3/MCP counters + any
    // future migration from the hand-written gauges above). The
    // facade renders only metrics that have been touched, so on a
    // cold server with no IMAP/POP3/MCP traffic this section is
    // empty — no leakage of unused-but-defined metric names.
    if let Some(ref h) = state.metrics_handle {
        let rendered = h.render();
        if !rendered.is_empty() {
            body.push('\n');
            body.push_str("# --- metrics-rs facade (IMAP/POP3/MCP + new code) ---\n");
            body.push_str(&rendered);
        }
    }
}
