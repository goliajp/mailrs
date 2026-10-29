//! Stopping a send that has not gone out yet.
//!
//! Until this existed the only way out of `Sending` was to wait: the
//! queue gives a message five days before it gives up, which is the
//! right rule for a remote that is merely down and the wrong one for a
//! message the sender no longer wants sent. Three invitations to
//! `@acme.test` — a reserved TLD that cannot resolve, so a DNS failure
//! on every attempt — sat in the queue for a day with no way to stop
//! them, which is what prompted this.
//!
//! ## What it does and does not promise
//!
//! It removes the jobs that have not been delivered. It cannot recall
//! a message an MX has already accepted, and it does not pretend to:
//! a recipient already marked delivered is left exactly as it is, and
//! the send comes back `partial` rather than `cancelled` when some
//! went and some did not. Cancelling something already sent is the one
//! answer a person must not be given falsely.

use super::*;

/// What a cancel actually stopped.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cancelled {
    /// Jobs taken out of the queue before anything was attempted.
    pub jobs_removed: usize,
    /// Recipients moved to a terminal "you cancelled this" state.
    pub recipients_cancelled: usize,
    /// Recipients an MX had already accepted. Left alone — the message
    /// is with them, and saying otherwise would be a lie the sender
    /// acts on.
    pub already_delivered: usize,
}

/// The text stored against a cancelled recipient.
///
/// Kept verbatim on the row like a remote's own words, because the
/// Send view renders whatever is there and "cancelled" with no
/// explanation reads as a failure nobody can account for.
pub const CANCELLED_REASON: &str = "cancelled before delivery";

/// Remove every not-yet-delivered job for `send_id` from the queue.
///
/// Returns the ids it removed. A job waits in one of three places —
/// `pending-idx`, [`SCHEDULED_IDX`] until its time, [`RETRY_IDX`] between
/// attempts — and all three are only read here, never rewritten: the
/// pending list has no `LREM`, and draining and refilling it raced every
/// concurrent send, whose job could fall out of the index between the
/// read and the refill. Each job is removed by [`cancel_waiting`].
pub fn drop_queued_jobs(conn: &mut kevy_client::Connection, send_id: &str) -> Vec<i64> {
    let mut candidates = conn.lrange(PENDING_IDX, 0, -1).unwrap_or_default();
    for z in [SCHEDULED_IDX, RETRY_IDX] {
        candidates.extend(conn.zrange(z, 0, -1).unwrap_or_default());
    }
    let mut removed = Vec::new();
    for m in candidates {
        let Some(id) = String::from_utf8_lossy(&m).parse::<i64>().ok() else {
            continue;
        };
        if removed.contains(&id) || !job_belongs_to(conn, id, send_id) {
            continue;
        }
        if cancel_waiting(conn, id).unwrap_or(false) {
            removed.push(id);
        }
    }
    removed
}

/// Take job `id` out of the queue if, and only if, it is still waiting.
///
/// The state is checked under `WATCH` and the job removed in the same
/// transaction, so a sender that claims it first wins and this answers
/// `false`: a job in flight may already have been accepted by an MX, and
/// "too late" is the honest answer. The id may stay on `pending-idx`; the
/// sender skips an id with no job row, and recovery only requeues rows
/// that exist.
pub fn cancel_waiting(conn: &mut kevy_client::Connection, id: i64) -> std::io::Result<bool> {
    let job_k = job_key(id);
    conn.watch(&[job_k.as_bytes()])?;
    if conn.hget(job_k.as_bytes(), b"state")?.as_deref() != Some(b"pending") {
        conn.unwatch()?;
        return Ok(false);
    }
    let id_s = id.to_string();
    let legacy = legacy_row_key(id);
    let mut tx = conn.multi()?;
    tx.queue(&[b"DEL", job_k.as_bytes(), legacy.as_bytes()])?;
    tx.queue(&[b"ZREM", SCHEDULED_IDX, id_s.as_bytes()])?;
    tx.queue(&[b"ZREM", RETRY_IDX, id_s.as_bytes()])?;
    Ok(tx.exec_watched()?.is_some())
}

/// Whether job `id` is part of `send_id`.
///
/// Reads the job hash's blob, which is where `send_id` lives — the
/// queue's own row has no column for it.
fn job_belongs_to(conn: &mut kevy_client::Connection, id: i64, send_id: &str) -> bool {
    let Ok(Some(raw)) = conn.hget(job_key(id).as_bytes(), b"blob") else {
        return false;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return false;
    };
    v.get("send_id").and_then(|s| s.as_str()) == Some(send_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(c: &mut kevy_client::Connection, id: i64, send: &str, state: &str) {
        let blob = format!(r#"{{"send_id":"{send}"}}"#);
        c.hset(
            job_key(id).as_bytes(),
            &[
                (b"state" as &[u8], state.as_bytes()),
                (b"blob", blob.as_bytes()),
            ],
        )
        .unwrap();
    }

    /// WATCH and MULTI, which the in-process store lacks, so against a
    /// kevy-server. A job waits in one of three places; a cancel finds it
    /// in each, and leaves every other send's job where it was.
    #[tokio::test]
    async fn a_cancel_takes_a_waiting_job_from_wherever_it_waits_and_nothing_else() {
        let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        job(&mut c, 41, "s-cancel", "pending");
        c.lpush(PENDING_IDX, &[b"41"]).unwrap();
        job(&mut c, 42, "s-cancel", "pending");
        c.zadd(SCHEDULED_IDX, &[(9e9, b"42" as &[u8])]).unwrap();
        job(&mut c, 43, "s-cancel", "pending");
        c.zadd(RETRY_IDX, &[(1.0, b"43" as &[u8])]).unwrap();
        job(&mut c, 44, "s-other", "pending");
        c.lpush(PENDING_IDX, &[b"44"]).unwrap();

        let mut removed = drop_queued_jobs(&mut c, "s-cancel");
        removed.sort();
        assert_eq!(removed, vec![41, 42, 43]);
        for id in [41, 42, 43] {
            assert!(c.hget(job_key(id).as_bytes(), b"state").unwrap().is_none());
        }
        assert!(c.zrange(SCHEDULED_IDX, 0, -1).unwrap().is_empty());
        assert!(c.zrange(RETRY_IDX, 0, -1).unwrap().is_empty());
        // the other send is untouched, and the pending list was never
        // rewritten: its ids are all still there
        assert_eq!(
            c.hget(job_key(44).as_bytes(), b"state").unwrap().as_deref(),
            Some(b"pending" as &[u8])
        );
        assert_eq!(
            c.lrange(PENDING_IDX, 0, -1).unwrap(),
            vec![b"44".to_vec(), b"41".to_vec()]
        );
    }

    /// A job the sender has claimed may already be with an MX. It stays,
    /// and the cancel says so.
    #[tokio::test]
    async fn a_job_already_claimed_is_too_late_to_cancel() {
        let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
        let mut c = kevy_client::Connection::connect(&url).unwrap();
        job(&mut c, 51, "s-late", "inflight");
        assert!(!cancel_waiting(&mut c, 51).unwrap());
        assert!(drop_queued_jobs(&mut c, "s-late").is_empty());
        assert_eq!(
            c.hget(job_key(51).as_bytes(), b"state").unwrap().as_deref(),
            Some(b"inflight" as &[u8])
        );
    }

    /// A cancel is not a recall. The counts are three separate facts
    /// and the type keeps them apart, because collapsing "stopped" and
    /// "already gone" is the one error a sender acts on.
    #[test]
    fn the_three_outcomes_are_distinct() {
        let c = Cancelled {
            jobs_removed: 2,
            recipients_cancelled: 2,
            already_delivered: 1,
        };
        assert_ne!(c.jobs_removed, c.already_delivered);
        assert_eq!(
            Cancelled::default(),
            Cancelled {
                jobs_removed: 0,
                recipients_cancelled: 0,
                already_delivered: 0
            }
        );
    }
}
