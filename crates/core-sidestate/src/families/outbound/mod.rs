//! The outbound queue on the network kevy: the keys every process that
//! enqueues writes and the sender drains.
//!
//!   `mailrs:outbound:job:{id}`      hash {state, attempts, blob,
//!                                         created_at, updated_at,
//!                                         claimed_at?, last_error?}
//!   `mailrs:outbound:pending-idx`   list, LPUSH on enqueue / retry
//!   `mailrs:outbound:scheduled-idx` zset, scored by send time
//!   `mailrs:outbound:counter`       next id

use mailrs_core_api::method::outbound::{OutboundMessageWire, QueueStatus};

mod cancel;
mod retry_idx;
mod terminal;

pub use cancel::*;
pub use retry_idx::RETRY_IDX;
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

/// Ids of jobs that failed for good; the sender adds them.
pub const FAILED_SET: &[u8] = b"mailrs:outbound:failed";

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

/// Allocate an id and write a fresh pending job; returns the id.
///
/// Every enqueuer goes through this one primitive. Two writers hitting
/// the same key layout in different ways is how no send delivered before
/// 2.9.38: webapi wrote one layout and the sender read another.
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

/// Serialize a queue row's blob field the way the sender consumes it.
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
