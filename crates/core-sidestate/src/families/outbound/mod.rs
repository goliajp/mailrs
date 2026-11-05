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
mod failed;
mod keys;
pub mod pause;
mod terminal;
mod withheld;
mod withheld_draft;

pub use cancel::*;
pub use failed::*;
pub use keys::*;
pub use terminal::*;
pub use withheld::*;
pub use withheld_draft::RETURNED_AS_DRAFT;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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
    /// The signed-in account that sent it, when that is not `sender`: an
    /// SMTP submission's MAIL FROM may be any of the account's send-as
    /// addresses. The sender counts and pauses by this when present.
    pub account: Option<&'a str>,
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
    write_pending(conn, job, None, now)
}

/// [`write_fresh_pending`], once for `once` (see [`once_key`]): the id of
/// the job written now, or `None` when an earlier call with the same key
/// already queued it.
///
/// The marker goes in the same pipeline as the job, so a process that dies
/// before sending it leaves neither and the next call queues the job; one
/// that dies after leaves both. Not atomic against a second caller with the
/// same key at the same moment; a spool file has one drain.
pub fn write_fresh_pending_once(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    once: &str,
    now: i64,
) -> std::io::Result<Option<i64>> {
    let key = once_key(once);
    if conn.exists(&[key.as_bytes()])? > 0 {
        return Ok(None);
    }
    write_pending_marked(conn, job, None, Some(&key), now).map(Some)
}

/// The kind a security alert's job carries. The sender neither counts
/// nor holds it: an alert must leave even when its own account is paused.
pub const SECURITY_ALERT_KIND: &str = "security_alert";

/// A security alert's job — the alert mailer's and the complaint
/// evidence's, and nobody else's. No request a user makes reaches this:
/// a kind a sender could choose would let any account skip the pause.
pub fn write_security_alert_pending(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    now: i64,
) -> std::io::Result<i64> {
    write_pending(conn, job, Some(SECURITY_ALERT_KIND), now)
}

fn write_pending(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    kind: Option<&str>,
    now: i64,
) -> std::io::Result<i64> {
    write_pending_marked(conn, job, kind, None, now)
}

fn write_pending_marked(
    conn: &mut kevy_client::Connection,
    job: &FreshPending<'_>,
    kind: Option<&str>,
    once_key: Option<&str>,
    now: i64,
) -> std::io::Result<i64> {
    let FreshPending {
        sender,
        recipient,
        message_data_base64,
        scheduled_at,
        original_sender,
        send_id,
        account,
    } = *job;
    let id = conn.incr(COUNTER)?;
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
    let blob = row_blob(&row, kind, account);
    let job_k = job_key(id);
    let id_str = id.to_string();
    let now_str = now.to_string();
    let sched_score = scheduled_at.map(|t| t.to_string());
    let once_ttl = ONCE_TTL_SECS.to_string();
    conn.pipeline(|p| {
        if let Some(key) = once_key {
            p.cmd(&[
                b"SET",
                key.as_bytes(),
                id_str.as_bytes(),
                b"EX",
                once_ttl.as_bytes(),
            ]);
        }
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
fn row_blob(row: &OutboundMessageWire, kind: Option<&str>, account: Option<&str>) -> String {
    let mut blob = serde_json::json!({
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
    });
    if let Some(kind) = kind {
        blob["kind"] = kind.into();
    }
    if let Some(account) = account {
        blob["account"] = account.to_ascii_lowercase().into();
    }
    blob.to_string()
}

#[cfg(test)]
mod once_tests {
    use super::*;

    /// Against a kevy-server: the job is written with a pipeline, which the
    /// in-process store does not run.
    #[tokio::test]
    async fn a_job_written_once_is_written_once() {
        let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        let job = FreshPending {
            sender: "a@x.test",
            recipient: "b@y.test",
            message_data_base64: "",
            scheduled_at: None,
            original_sender: None,
            send_id: None,
            account: None,
        };
        let first = write_fresh_pending_once(&mut c, &job, "k1", 100).unwrap();
        assert!(first.is_some());
        assert_eq!(
            write_fresh_pending_once(&mut c, &job, "k1", 101).unwrap(),
            None
        );
        assert!(
            write_fresh_pending_once(&mut c, &job, "k2", 102)
                .unwrap()
                .is_some()
        );
        assert_eq!(c.lrange(PENDING_IDX, 0, -1).unwrap().len(), 2);
        let marker = c.get(once_key("k1").as_bytes()).unwrap();
        assert_eq!(marker, first.map(|id| id.to_string().into_bytes()));
    }
}
