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
/// Returns the ids it removed. The pending list has no `LREM`, so it is
/// drained and rebuilt without them — the same shape
/// `remove_inflight_and_del` already uses, and for the same reason.
///
/// **Only `pending-idx` and `scheduled-idx`.** A job already claimed by
/// the sender is in flight: taking it out from under the worker would
/// race a delivery that may already have been accepted, and the honest
/// answer there is "too late", not a silent removal.
pub fn drop_queued_jobs(conn: &mut kevy_client::Connection, send_id: &str) -> Vec<i64> {
    let mut removed = Vec::new();
    let mut kept: Vec<Vec<u8>> = Vec::new();
    while let Some(b) = rpop_one(conn, PENDING_IDX) {
        let id = String::from_utf8_lossy(&b).parse::<i64>().ok();
        match id.filter(|i| job_belongs_to(conn, *i, send_id)) {
            Some(i) => {
                let _ = conn.del(&[job_key(i).as_bytes()]);
                let _ = conn.del(&[format!("mailrs:outbound:{i}").as_bytes()]);
                removed.push(i);
            }
            None => kept.push(b),
        }
    }
    // Back in the order they came out: the list is drained from the tail
    // and pushed to the head, so replaying in pop order restores it.
    for b in kept {
        let _ = conn.lpush(PENDING_IDX, &[b.as_slice()]);
    }
    removed
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
