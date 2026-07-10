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

use crate::NetKevy;

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
    let blob = row_blob(row);
    let _ = conn.hset(
        format!("mailrs:outbound:{}", row.id).as_bytes(),
        &[(b"blob".as_slice(), blob.as_bytes())],
    );
}

pub async fn enqueue<S: NetKevy>(
    State(state): State<Arc<S>>,
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
    // v2.3 §P8-A (2026-07-09): batch the HSET + (ZADD or LPUSH) into
    // one pipeline. Both ops are non-CAS best-effort writes (their
    // pre-fix _ = ignored the error), so pipeline non-atomicity is
    // functionally equivalent — same crash-window as before, one
    // less RTT on the enqueue path.
    let blob = row_blob(&row);
    let key = format!("mailrs:outbound:{id}");
    let id_str = id.to_string();
    let sched_score = req.scheduled_at.map(|t| t.to_string());
    let _ = conn.pipeline(|p| {
        p.cmd(&[b"HSET", key.as_bytes(), b"blob", blob.as_bytes()]);
        match sched_score.as_deref() {
            Some(score) => {
                p.cmd(&[
                    b"ZADD",
                    b"mailrs:outbound:scheduled",
                    score.as_bytes(),
                    id_str.as_bytes(),
                ]);
            }
            None => {
                p.cmd(&[b"LPUSH", b"mailrs:outbound:pending", id_str.as_bytes()]);
            }
        }
    });
    Ok(Json(EnqueueResponse { id }))
}

/// Serialize a queue row's blob field the way the sender consumes it.
/// Extracted from `write_row` so both the single-op helper and the
/// pipeline path can share the shape without diverging.
fn row_blob(row: &OutboundMessageWire) -> String {
    serde_json::json!({
        "id": row.id, "sender": row.sender, "recipient": row.recipient,
        "original_sender": row.original_sender,
        "message_data_b64": row.message_data_base64,
        "status": format!("{:?}", row.status).to_lowercase(),
        "attempts": row.attempts, "last_error": row.last_error,
        "next_retry": row.next_retry, "scheduled_at": row.scheduled_at,
        "created_at": row.created_at, "updated_at": row.updated_at,
    })
    .to_string()
}

pub async fn claim<S: NetKevy>(
    State(state): State<Arc<S>>,
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

pub async fn stats<S: NetKevy>(State(state): State<Arc<S>>) -> Json<QueueStatsResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(QueueStatsResponse {
            pending: 0,
            inflight: 0,
            delivered: 0,
            failed: 0,
            bounced: 0,
        });
    };
    // v2.3 §P8-A (2026-07-09): batched read — 5 sequential RTTs
    // (LLEN×2 + GET×3) → 1 RTT via kevy-client 1.14 pipeline.
    // Reply positions are indexed against the queued command order.
    // Any per-reply Error / Nil / shape mismatch falls back to `0`
    // — the pre-fix version's `.unwrap_or(0)` had the same contract.
    let replies = conn
        .pipeline(|p| {
            p.cmd(&[b"LLEN", b"mailrs:outbound:pending"]);
            p.cmd(&[b"LLEN", b"mailrs:outbound:inflight"]);
            p.cmd(&[b"GET", b"mailrs:outbound:delivered:count"]);
            p.cmd(&[b"GET", b"mailrs:outbound:failed:count"]);
            p.cmd(&[b"GET", b"mailrs:outbound:bounced:count"]);
        })
        .unwrap_or_default();
    fn int_at(replies: &[kevy_client::Reply], i: usize) -> i64 {
        match replies.get(i) {
            Some(kevy_client::Reply::Int(n)) => *n,
            _ => 0,
        }
    }
    fn cnt_at(replies: &[kevy_client::Reply], i: usize) -> i64 {
        match replies.get(i) {
            Some(kevy_client::Reply::Bulk(b)) => std::str::from_utf8(b)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            _ => 0,
        }
    }
    Json(QueueStatsResponse {
        pending: int_at(&replies, 0),
        inflight: int_at(&replies, 1),
        delivered: cnt_at(&replies, 2),
        failed: cnt_at(&replies, 3),
        bounced: cnt_at(&replies, 4),
    })
}

pub async fn recover_stale<S: NetKevy>(
    State(state): State<Arc<S>>,
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

pub async fn mark_delivered<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(id): Path<i64>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    remove_inflight_and_del(&mut conn, id);
    let _ = conn.incr(b"mailrs:outbound:delivered:count");
    StatusCode::NO_CONTENT
}

pub async fn mark_failed<S: NetKevy>(
    State(state): State<Arc<S>>,
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

pub async fn mark_bounced<S: NetKevy>(
    State(state): State<Arc<S>>,
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
