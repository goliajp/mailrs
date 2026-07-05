//! Outbound-queue RPC served from the network kevy — the same
//! `mailrs:outbound:*` keys the sender drains and webapi enqueues to, so
//! the queue is identical regardless of which core serves it:
//!   `mailrs:outbound:{id}`          hash, field `blob` = JSON row
//!   `mailrs:outbound:pending`       list of ids (LPUSH / RPOP)
//!   `mailrs:outbound:inflight`      list of claimed ids
//!   `mailrs:outbound:scheduled`     zset id scored by send-time
//!   `mailrs:outbound:suppression`   set of bounced recipients
//!   `mailrs:outbound:counter`       next id
//!   `mailrs:outbound:{delivered,failed,bounced}:count`  status tallies
//!
//! Status/attempts (absent from the loose enqueue blob) are tracked in the
//! blob + the count keys so `stats` matches the pg-core table counts.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use mailrs_core_api::method::outbound::{
    ClaimRequest, ClaimResponse, EnqueueRequest, EnqueueResponse, MarkBouncedRequest,
    MarkFailedRequest, OutboundMessageWire, QueueStatsResponse, QueueStatus, RecoverStaleRequest,
    RecoverStaleResponse,
};

use crate::FastcoreState;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// RPOP one element (kevy_client's `rpop` takes a count and returns a Vec).
fn rpop_one(conn: &mut kevy_client::Connection, key: &[u8]) -> Option<Vec<u8>> {
    conn.rpop(key, 1).ok().and_then(|v| v.into_iter().next())
}

/// Read `mailrs:outbound:{id}` blob → OutboundMessageWire (loose blob
/// fields + synthesized status/attempts defaults).
fn read_row(conn: &mut kevy_client::Connection, id: i64) -> Option<OutboundMessageWire> {
    let raw = conn
        .hget(format!("mailrs:outbound:{id}").as_bytes(), b"blob")
        .ok()
        .flatten()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    Some(OutboundMessageWire {
        id,
        sender: v.get("sender")?.as_str()?.to_string(),
        recipient: v.get("recipient")?.as_str()?.to_string(),
        original_sender: v
            .get("original_sender")
            .and_then(|x| x.as_str())
            .map(String::from),
        message_data_base64: v
            .get("message_data_b64")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        status: match v.get("status").and_then(|x| x.as_str()) {
            Some("inflight") => QueueStatus::Inflight,
            Some("delivered") => QueueStatus::Delivered,
            Some("failed") => QueueStatus::Failed,
            Some("bounced") => QueueStatus::Bounced,
            _ => QueueStatus::Pending,
        },
        attempts: v.get("attempts").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        last_error: v
            .get("last_error")
            .and_then(|x| x.as_str())
            .map(String::from),
        next_retry: v.get("next_retry").and_then(|x| x.as_i64()),
        scheduled_at: v.get("scheduled_at").and_then(|x| x.as_i64()),
        created_at: v.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0),
        updated_at: v.get("updated_at").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

fn write_row(conn: &mut kevy_client::Connection, row: &OutboundMessageWire) {
    let blob = serde_json::json!({
        "id": row.id, "sender": row.sender, "recipient": row.recipient,
        "original_sender": row.original_sender,
        "message_data_b64": row.message_data_base64,
        "status": format!("{:?}", row.status).to_lowercase(),
        "attempts": row.attempts, "last_error": row.last_error,
        "next_retry": row.next_retry, "scheduled_at": row.scheduled_at,
        "created_at": row.created_at, "updated_at": row.updated_at,
    })
    .to_string();
    let _ = conn.hset(
        format!("mailrs:outbound:{}", row.id).as_bytes(),
        &[(b"blob".as_slice(), blob.as_bytes())],
    );
}

pub async fn enqueue(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<EnqueueRequest>,
) -> Result<Json<EnqueueResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = conn
        .incr(b"mailrs:outbound:counter")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = now_secs();
    let row = OutboundMessageWire {
        id,
        sender: req.sender,
        recipient: req.recipient,
        original_sender: req.original_sender,
        message_data_base64: req.message_data_base64,
        status: QueueStatus::Pending,
        attempts: 0,
        last_error: None,
        next_retry: None,
        scheduled_at: req.scheduled_at,
        created_at: now,
        updated_at: now,
    };
    write_row(&mut conn, &row);
    match req.scheduled_at {
        Some(t) => {
            let _ = conn.zadd(
                b"mailrs:outbound:scheduled",
                &[(t as f64, id.to_string().as_bytes())],
            );
        }
        None => {
            let _ = conn.lpush(b"mailrs:outbound:pending", &[id.to_string().as_bytes()]);
        }
    }
    Ok(Json(EnqueueResponse { id }))
}

pub async fn claim(
    State(state): State<Arc<FastcoreState>>,
    Json(req): Json<ClaimRequest>,
) -> Json<ClaimResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(ClaimResponse { items: Vec::new() });
    };
    let mut items = Vec::new();
    for _ in 0..req.batch_size {
        let Some(id_bytes) = rpop_one(&mut conn, b"mailrs:outbound:pending") else {
            break;
        };
        let Ok(id) = String::from_utf8_lossy(&id_bytes).parse::<i64>() else {
            continue;
        };
        let _ = conn.lpush(b"mailrs:outbound:inflight", &[id.to_string().as_bytes()]);
        if let Some(mut row) = read_row(&mut conn, id) {
            row.status = QueueStatus::Inflight;
            row.attempts += 1;
            row.updated_at = now_secs();
            write_row(&mut conn, &row);
            items.push(row);
        }
    }
    Json(ClaimResponse { items })
}

pub async fn stats(State(state): State<Arc<FastcoreState>>) -> Json<QueueStatsResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(QueueStatsResponse {
            pending: 0,
            inflight: 0,
            delivered: 0,
            failed: 0,
            bounced: 0,
        });
    };
    let cnt = |conn: &mut kevy_client::Connection, k: &str| -> i64 {
        conn.get(k.as_bytes())
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8_lossy(&v).parse().ok())
            .unwrap_or(0)
    };
    Json(QueueStatsResponse {
        pending: conn.llen(b"mailrs:outbound:pending").unwrap_or(0) as i64,
        inflight: conn.llen(b"mailrs:outbound:inflight").unwrap_or(0) as i64,
        delivered: cnt(&mut conn, "mailrs:outbound:delivered:count"),
        failed: cnt(&mut conn, "mailrs:outbound:failed:count"),
        bounced: cnt(&mut conn, "mailrs:outbound:bounced:count"),
    })
}

pub async fn recover_stale(
    State(state): State<Arc<FastcoreState>>,
    Json(_req): Json<RecoverStaleRequest>,
) -> Json<RecoverStaleResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(RecoverStaleResponse { recovered: 0 });
    };
    // move every inflight id back to pending (no per-item timestamp in the
    // kevy model, so recover-stale reclaims the whole inflight list)
    let mut recovered = 0u32;
    while let Some(id_bytes) = rpop_one(&mut conn, b"mailrs:outbound:inflight") {
        let _ = conn.lpush(b"mailrs:outbound:pending", &[id_bytes.as_slice()]);
        recovered += 1;
    }
    Json(RecoverStaleResponse { recovered })
}

fn remove_inflight_and_del(conn: &mut kevy_client::Connection, id: i64) {
    // kevy_client has no LREM; rebuild the inflight list without `id`.
    let mut kept = Vec::new();
    while let Some(b) = rpop_one(conn, b"mailrs:outbound:inflight") {
        if String::from_utf8_lossy(&b).parse::<i64>().ok() != Some(id) {
            kept.push(b);
        }
    }
    for b in kept {
        let _ = conn.lpush(b"mailrs:outbound:inflight", &[b.as_slice()]);
    }
    let _ = conn.del(&[format!("mailrs:outbound:{id}").as_bytes()]);
}

pub async fn mark_delivered(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    remove_inflight_and_del(&mut conn, id);
    let _ = conn.incr(b"mailrs:outbound:delivered:count");
    StatusCode::NO_CONTENT
}

pub async fn mark_failed(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
    Json(req): Json<MarkFailedRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    // retry: back to pending, record the error on the row
    if let Some(mut row) = read_row(&mut conn, id) {
        row.status = QueueStatus::Pending;
        row.last_error = Some(req.error);
        row.next_retry = req.next_retry;
        row.updated_at = now_secs();
        write_row(&mut conn, &row);
    }
    // pull from inflight, push back to pending
    let mut kept = Vec::new();
    let mut found = false;
    while let Some(b) = rpop_one(&mut conn, b"mailrs:outbound:inflight") {
        if String::from_utf8_lossy(&b).parse::<i64>().ok() == Some(id) {
            found = true;
        } else {
            kept.push(b);
        }
    }
    for b in kept {
        let _ = conn.lpush(b"mailrs:outbound:inflight", &[b.as_slice()]);
    }
    if found {
        let _ = conn.lpush(b"mailrs:outbound:pending", &[id.to_string().as_bytes()]);
    }
    let _ = conn.incr(b"mailrs:outbound:failed:count");
    StatusCode::NO_CONTENT
}

pub async fn mark_bounced(
    State(state): State<Arc<FastcoreState>>,
    Path(id): Path<i64>,
    Json(req): Json<MarkBouncedRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if let Some(row) = read_row(&mut conn, id) {
        let _ = conn.sadd(b"mailrs:outbound:suppression", &[row.recipient.as_bytes()]);
    }
    let _ = req.error;
    remove_inflight_and_del(&mut conn, id);
    let _ = conn.incr(b"mailrs:outbound:bounced:count");
    StatusCode::NO_CONTENT
}
