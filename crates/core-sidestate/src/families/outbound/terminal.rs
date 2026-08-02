//! The terminal marks and the sweep that reclaims a stale claim.

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
    MarkBouncedRequest, MarkFailedRequest, QueueStatus, RecoverStaleRequest, RecoverStaleResponse,
};

use super::*;
use crate::NetKevy;

pub fn remove_inflight_and_del(conn: &mut kevy_client::Connection, id: i64) {
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
pub fn dual_write_terminal(conn: &mut kevy_client::Connection, id: i64, state: &[u8]) {
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
