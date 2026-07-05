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
    ReactionAggregateRow, ReactionsResponse, ToggleReactionRequest, WebhookListResponse,
    WebhookSubWire,
};

use crate::FastcoreState;

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
            me: users.iter().any(|u| *u == user),
        });
        i += 2;
    }
    rows
}

pub async fn get_thread_reactions(
    State(state): State<Arc<FastcoreState>>,
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

pub async fn toggle_reaction(
    State(state): State<Arc<FastcoreState>>,
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

pub async fn create_webhook(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<CreateWebhookRequest>,
) -> Result<Json<CreateWebhookResponse>, StatusCode> {
    use base64::Engine as _;
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = conn
        .incr(b"admin:webhooks:counter")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut secret_bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut secret_bytes);
    let signing_secret = base64::engine::general_purpose::STANDARD.encode(secret_bytes);
    let w = WebhookSubWire {
        id,
        account_address: req.account_address.clone(),
        url: req.url,
        event_type: req.event_type,
        filter_sender: req.filter_sender,
        filter_thread_id: req.filter_thread_id,
        signing_secret: signing_secret.clone(),
        active: true,
        created_at: now_secs(),
    };
    let json = serde_json::to_vec(&w).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.hset(
        format!("admin:webhooks:{}", req.account_address).as_bytes(),
        &[(id.to_string().as_bytes(), json.as_slice())],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(CreateWebhookResponse { id, signing_secret }))
}

pub async fn list_webhooks(
    State(state): State<Arc<FastcoreState>>,
    Path(address): Path<String>,
) -> Json<WebhookListResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(WebhookListResponse { items: Vec::new() });
    };
    let flat = conn
        .hgetall(format!("admin:webhooks:{address}").as_bytes())
        .unwrap_or_default();
    let items = flat
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| if i % 2 == 1 { Some(v) } else { None })
        .filter_map(|v| serde_json::from_slice::<WebhookSubWire>(&v).ok())
        .collect();
    Json(WebhookListResponse { items })
}

pub async fn delete_webhook(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    // webhooks are keyed by account; scan the account index (same as webapi)
    let id_str = id.to_string();
    let addrs = conn.smembers(b"mailrs:accounts:index").unwrap_or_default();
    for addr_bytes in addrs {
        if let Ok(addr) = String::from_utf8(addr_bytes) {
            let _ = conn.hdel(
                format!("admin:webhooks:{addr}").as_bytes(),
                &[id_str.as_bytes()],
            );
        }
    }
    StatusCode::NO_CONTENT
}

// ── audit log ───────────────────────────────────────────────────────

pub async fn list_audit_log(
    State(state): State<Arc<FastcoreState>>,
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
