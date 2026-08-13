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
use axum::extract::State;
use axum::http::StatusCode;

use mailrs_core_api::method::outbound::{
    ClaimRequest, ClaimResponse, EnqueueRequest, EnqueueResponse, OutboundMessageWire,
    QueueStatsResponse, QueueStatus,
};

use crate::NetKevy;

mod terminal;

pub use terminal::*;

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

/// The queue the sender pops from.
///
/// `pub` alongside [`SCHEDULED_IDX`] so the sender names the same
/// bytes rather than its own copy of the string — see below.
pub const PENDING_IDX: &[u8] = b"mailrs:outbound:pending-idx";

/// Where a future-dated send waits, scored by its send time.
///
/// `pub` because the sender's due-sweep had a second constant of its
/// own — `mailrs:outbound:scheduled`, without the suffix — and swept
/// that instead. Nothing has written that key since the queue moved to
/// the `-idx` names in v2.5.3, so a scheduled message was enqueued
/// here, never promoted, and never left. Both prod zsets were empty
/// when this was found, so no mail was lost; the feature had simply
/// never run. One exported name, and the two cannot drift again.
pub const SCHEDULED_IDX: &[u8] = b"mailrs:outbound:scheduled-idx";

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
        send_id: v.get("send_id").and_then(|x| x.as_str()).map(String::from),
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
/// One outbound job to enqueue.
///
/// Named fields rather than positional arguments because four of them
/// are string-ish and adjacent — `sender`, `recipient`,
/// `original_sender`, `send_id`. Swapping any two compiles cleanly and
/// silently misroutes mail; clippy flagged the arity and it was right,
/// so this removes the class rather than silencing the lint.
pub struct FreshPending<'a> {
    /// Envelope sender (SMTP MAIL FROM).
    pub sender: &'a str,
    /// Envelope recipient — one job per recipient.
    pub recipient: &'a str,
    pub message_data_base64: &'a str,
    /// Future send time; `None` delivers as soon as the sender claims it.
    pub scheduled_at: Option<i64>,
    /// Pre-SRS sender for forwarded mail.
    pub original_sender: Option<&'a str>,
    /// The Send this job belongs to, when a user composed it. `None` for
    /// generated mail — tls-rpt reports, bounce DSNs, sieve redirects —
    /// which must not appear in anyone's Send list.
    pub send_id: Option<&'a str>,
}

pub fn write_fresh_pending(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    now: i64,
) -> std::io::Result<i64> {
    let FreshPending {
        sender,
        recipient,
        message_data_base64,
        scheduled_at,
        original_sender,
        send_id,
    } = *job;
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
        send_id: send_id.map(String::from),
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
        &FreshPending {
            sender: &req.sender,
            recipient: &req.recipient,
            message_data_base64: &req.message_data_base64,
            scheduled_at: req.scheduled_at,
            original_sender: req.original_sender.as_deref(),
            // No Send row: this RPC enqueues mail nobody composed in the
            // UI (tls-rpt, bounces). A missing group is the honest
            // answer, not an invented one.
            send_id: None,
        },
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
        // Without this the field round-trips as None and the sender can
        // never find the Send row to report an outcome against.
        "send_id": row.send_id,
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

#[cfg(test)]
mod key_tests {
    use super::*;

    /// The suffix is the whole bug.
    ///
    /// The sender's due-sweep carried its own copy of this name without
    /// the `-idx`, so it walked a zset nothing writes: a scheduled send
    /// was enqueued here and never promoted. Asserting the bytes is
    /// weak on its own — what makes it hold is that every reader now
    /// imports these two constants and no file spells them again.
    #[test]
    fn the_queue_names_are_the_ones_every_reader_imports() {
        assert_eq!(PENDING_IDX, b"mailrs:outbound:pending-idx");
        assert_eq!(SCHEDULED_IDX, b"mailrs:outbound:scheduled-idx");
    }
}
