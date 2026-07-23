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
//!
//! v2.5.1 §P8-B-A (roadmap Phase 6.2) — introduces a **dual-write** to
//! the new single-hash job FSM layout described in
//! `.claude/rfcs/20260709-v2.3-p8b-outbound-job-state-fsm.md`:
//!
//!   `mailrs:outbound:job:{id}`      hash {state, attempts, blob,
//!                                         created_at, updated_at,
//!                                         claimed_at?, last_error?}
//!   `mailrs:outbound:pending-idx`   list  (LPUSH on enqueue-pending
//!                                          / retry; drained parallel
//!                                          to old pending list on claim)
//!   `mailrs:outbound:scheduled-idx` zset  (score=scheduled_at)
//!   `mailrs:outbound:done-idx`      list  (LPUSH on any terminal
//!                                          transition)
//!
//! Every write path in this file now performs the equivalent op on the
//! new keys after the existing legacy op — best-effort, `let _ =`
//! ignored just like the legacy path. Reads still hit the legacy
//! layout; Phase 6.3 (v2.5.2 read cutover) will swap them.

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

/// v2.5.1 §P8-B-A dual-write keyspace.
fn job_key(id: i64) -> String {
    format!("mailrs:outbound:job:{id}")
}
const PENDING_IDX: &[u8] = b"mailrs:outbound:pending-idx";
const SCHEDULED_IDX: &[u8] = b"mailrs:outbound:scheduled-idx";
const DONE_IDX: &[u8] = b"mailrs:outbound:done-idx";

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

/// Allocate an id and write a fresh pending job.
///
/// Extracted from the axum handler so callers that already own a
/// `kevy_client::Connection` (the webapi's `/api/mail/send` path)
/// enqueue through the same primitive as the RPC route. Two writers
/// hitting the same key layout in different ways is exactly how the
/// pre-2.9.38 mismatch happened (webapi was writing legacy
/// `mailrs:outbound:{id}` + `pending`, sender was reading v2
/// `mailrs:outbound:job:{id}` + `pending-idx`, so no send delivered).
///
/// Returns the allocated id.
pub fn write_fresh_pending(
    conn: &mut kevy_client::Connection,
    sender: &str,
    recipient: &str,
    message_data_base64: &str,
    scheduled_at: Option<i64>,
    original_sender: Option<&str>,
    now: i64,
) -> std::io::Result<i64> {
    let id = conn.incr(b"mailrs:outbound:counter")?;
    let row = OutboundMessageWire {
        id,
        sender: sender.to_string(),
        recipient: recipient.to_string(),
        original_sender: original_sender.map(String::from),
        message_data_base64: message_data_base64.to_string(),
        status: QueueStatus::Pending,
        attempts: 0,
        last_error: None,
        next_retry: None,
        scheduled_at,
        created_at: now,
        updated_at: now,
    };
    let blob = row_blob(&row);
    let job_k = job_key(id);
    let id_str = id.to_string();
    let now_str = now.to_string();
    let sched_score = scheduled_at.map(|t| t.to_string());
    conn.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            b"pending",
            b"attempts",
            b"0",
            b"blob",
            blob.as_bytes(),
            b"created_at",
            now_str.as_bytes(),
            b"updated_at",
            now_str.as_bytes(),
        ]);
        match sched_score.as_deref() {
            Some(score) => {
                p.cmd(&[b"ZADD", SCHEDULED_IDX, score.as_bytes(), id_str.as_bytes()]);
            }
            None => {
                p.cmd(&[b"LPUSH", PENDING_IDX, id_str.as_bytes()]);
            }
        }
    })?;
    Ok(id)
}

/// Put an existing job back on the pending index — the retry primitive.
///
/// Sets state=pending on the job hash and LPUSHes pending-idx so the
/// next sender BRPOP picks it up. Matches the semantics of the sender's
/// own `dual_write_pending` retry helper. Silently no-ops if the job
/// hash doesn't exist (nothing sensible to requeue).
pub fn requeue_pending(
    conn: &mut kevy_client::Connection,
    id: i64,
    now: i64,
) -> std::io::Result<bool> {
    let job_k = job_key(id);
    if conn.exists(&[job_k.as_bytes()])? == 0 {
        return Ok(false);
    }
    let id_str = id.to_string();
    let now_str = now.to_string();
    conn.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            b"pending",
            b"updated_at",
            now_str.as_bytes(),
        ]);
        p.cmd(&[b"LPUSH", PENDING_IDX, id_str.as_bytes()]);
    })?;
    Ok(true)
}

pub async fn enqueue<S: NetKevy>(
    State(state): State<Arc<S>>,
    Json(req): Json<EnqueueRequest>,
) -> Result<Json<EnqueueResponse>, StatusCode> {
    let mut conn = state.net_conn().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let id = write_fresh_pending(
        &mut conn,
        &req.sender,
        &req.recipient,
        &req.message_data_base64,
        req.scheduled_at,
        req.original_sender.as_deref(),
        now_secs(),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        // v2.5.1 §P8-B-A dual-write: mirror state=inflight on the new
        // job hash + drain a matching entry from pending-idx so the two
        // indexes stay length-consistent for the Phase 6.3 read cutover.
        let now_str = now_secs().to_string();
        let job_k = job_key(id);
        let _ = conn.pipeline(|p| {
            p.cmd(&[
                b"HSET",
                job_k.as_bytes(),
                b"state",
                b"inflight",
                b"claimed_at",
                now_str.as_bytes(),
                b"updated_at",
                now_str.as_bytes(),
            ]);
            p.cmd(&[b"HINCRBY", job_k.as_bytes(), b"attempts", b"1"]);
        });
        let _ = rpop_one(&mut conn, PENDING_IDX);
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
    // v2.5.3 §P8-B-C (Phase 8.2): `pending` from `pending-idx` llen —
    // may over-count while duplicate LPUSH entries drain (see Phase 8.1
    // memory), but converges to the true pending count once the sender
    // dedupes them. `inflight` is deprecated in the v2 layout (sender
    // no longer LPUSHes the legacy list; a truly precise count would
    // need a job-hash SCAN which is too expensive for a stats
    // endpoint) — returned as 0. Terminal counters still read from
    // the legacy counter keys because the webapi RPC mark_* path
    // continues to INCR them.
    let replies = conn
        .pipeline(|p| {
            p.cmd(&[b"LLEN", b"mailrs:outbound:pending-idx"]);
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
        inflight: 0,
        delivered: cnt_at(&replies, 1),
        failed: cnt_at(&replies, 2),
        bounced: cnt_at(&replies, 3),
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
    dual_write_terminal(&mut conn, id, b"delivered");
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
    // v2.5.1 §P8-B-A dual-write: `mark_failed` is a retry (state back
    // to pending, not terminal). Mirror the new hash + pending-idx.
    let now_str = now_secs().to_string();
    let job_k = job_key(id);
    let _ = conn.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            b"pending",
            b"updated_at",
            now_str.as_bytes(),
        ]);
        p.cmd(&[b"HDEL", job_k.as_bytes(), b"claimed_at"]);
        p.cmd(&[b"LPUSH", PENDING_IDX, id.to_string().as_bytes()]);
    });
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
    dual_write_terminal(&mut conn, id, b"bounced");
    StatusCode::NO_CONTENT
}

/// v2.5.1 §P8-B-A helper: terminal transition on the new job hash.
/// `state` = b"delivered" | b"bounced" | b"failed" (no-retry).
/// The row hash + counter are already handled by the legacy path;
/// this only mirrors the FSM state + done-idx tail + 24h TTL.
fn dual_write_terminal(conn: &mut kevy_client::Connection, id: i64, state: &[u8]) {
    let now_str = now_secs().to_string();
    let id_str = id.to_string();
    let job_k = job_key(id);
    let _ = conn.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            state,
            b"updated_at",
            now_str.as_bytes(),
        ]);
        p.cmd(&[b"HDEL", job_k.as_bytes(), b"claimed_at"]);
        p.cmd(&[b"LPUSH", DONE_IDX, id_str.as_bytes()]);
        // 24 h retention on the terminal-state hash — enough for
        // post-mortem inspection without ballooning AOF (per RFC §9).
        p.cmd(&[b"EXPIRE", job_k.as_bytes(), b"86400"]);
    });
}
