//! Reactions / webhooks / audit-log — shared admin/thread side-state
//! served from the network kevy, keyed exactly like webapi + pg-core:
//!   `reactions_index:{thread_id}`   set of uids that carry ≥1 reaction
//!   `reactions:{thread_id}:{uid}`   hash emoji → CSV of users
//!   `admin:webhooks:{address}`      hash id → JSON WebhookSubWire
//!   `admin:webhooks:counter`        string next id
//!   `admin:audit_log`               list of JSON AuditRowWire (newest first)

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use mailrs_core_api::method::admin::{
    AuditListResponse, AuditRowWire, CreateWebhookRequest, CreateWebhookResponse, ListAuditQuery,
    LogAuditRequest, ReactionAggregateRow, ReactionsResponse, ToggleReactionRequest,
    WebhookListResponse,
};

use crate::NetKevy;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── reactions ───────────────────────────────────────────────────────

/// Aggregate one message's reaction hash (emoji → CSV) into rows.
fn aggregate(
    conn: &mut kevy_client::Connection,
    thread_id: &str,
    uid: i64,
    user: &str,
) -> Vec<ReactionAggregateRow> {
    let flat = conn
        .hgetall(format!("reactions:{thread_id}:{uid}").as_bytes())
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let emoji = String::from_utf8_lossy(&flat[i]).to_string();
        let csv = String::from_utf8_lossy(&flat[i + 1]).to_string();
        let users: Vec<&str> = csv.split(',').filter(|s| !s.is_empty()).collect();
        rows.push(ReactionAggregateRow {
            message_uid: uid,
            emoji,
            count: users.len() as i64,
            me: users.contains(&user),
        });
        i += 2;
    }
    rows
}

pub async fn get_thread_reactions<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, thread_id)): Path<(String, String)>,
) -> Json<ReactionsResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(ReactionsResponse {
            reactions: Vec::new(),
        });
    };
    let uids = conn
        .smembers(format!("reactions_index:{thread_id}").as_bytes())
        .unwrap_or_default();
    let mut reactions = Vec::new();
    for uid_bytes in uids {
        if let Ok(uid) = String::from_utf8_lossy(&uid_bytes).parse::<i64>() {
            reactions.extend(aggregate(&mut conn, &thread_id, uid, &user));
        }
    }
    Json(ReactionsResponse { reactions })
}

pub async fn toggle_reaction<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, thread_id, uid)): Path<(String, String, i64)>,
    Json(req): Json<ToggleReactionRequest>,
) -> Result<Json<ReactionsResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let key = format!("reactions:{thread_id}:{uid}");
    let index_key = format!("reactions_index:{thread_id}");
    let cur = conn
        .hget(key.as_bytes(), req.emoji.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .unwrap_or_default();
    let mut users: Vec<String> = String::from_utf8_lossy(&cur)
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if users.contains(&user) {
        users.retain(|u| u != &user);
    } else {
        users.push(user.clone());
    }
    let joined = users.join(",");
    if joined.is_empty() {
        let _ = conn.hdel(key.as_bytes(), &[req.emoji.as_bytes()]);
    } else {
        let _ = conn.hset(key.as_bytes(), &[(req.emoji.as_bytes(), joined.as_bytes())]);
    }
    let remaining = conn.hlen(key.as_bytes()).unwrap_or(0);
    let uid_bytes = uid.to_string();
    if remaining > 0 {
        let _ = conn.sadd(index_key.as_bytes(), &[uid_bytes.as_bytes()]);
    } else {
        let _ = conn.srem(index_key.as_bytes(), &[uid_bytes.as_bytes()]);
    }
    Ok(Json(ReactionsResponse {
        reactions: aggregate(&mut conn, &thread_id, uid, &user),
    }))
}

// ── webhooks ────────────────────────────────────────────────────────

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

// ── audit log ───────────────────────────────────────────────────────

pub async fn list_audit_log<S: NetKevy>(
    State(state): State<Arc<S>>,
    Query(q): Query<ListAuditQuery>,
) -> Json<AuditListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(AuditListResponse { items: Vec::new() });
    };
    let limit = q.limit.max(1) as i64;
    let entries = conn
        .lrange(b"admin:audit_log", 0, limit - 1)
        .unwrap_or_default();
    let items = entries
        .into_iter()
        .filter_map(|v| serde_json::from_slice::<AuditRowWire>(&v).ok())
        .collect();
    Json(AuditListResponse { items })
}

pub async fn log_audit<S: NetKevy>(
    State(state): State<Arc<S>>,
    Json(req): Json<LogAuditRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let id = conn.incr(b"admin:audit_log:counter").unwrap_or(0);
    let row = AuditRowWire {
        id,
        timestamp: now_secs(),
        actor: req.actor,
        action: req.action,
        target: req.target,
        detail: req.detail,
    };
    if let Ok(json) = serde_json::to_vec(&row) {
        // newest-first: LPUSH so lrange 0.. returns recent entries first
        let _ = conn.lpush(b"admin:audit_log", &[json.as_slice()]);
    }
    StatusCode::NO_CONTENT
}
