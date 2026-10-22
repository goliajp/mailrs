//! Every outbound-queue key, spelled once.
//!
//! The sender, the webapi, intake and both cores all touch these. Each
//! used to carry its own copy of the strings, and one copy drifting is
//! how a scheduled send sat in a zset nothing swept (see
//! [`SCHEDULED_IDX`]).

/// The job hash the sender drives through its states.
pub fn job_key(id: impl std::fmt::Display) -> String {
    format!("mailrs:outbound:job:{id}")
}

/// What every [`job_key`] starts with.
pub const JOB_PREFIX: &str = "mailrs:outbound:job:";

/// SCAN pattern over every job hash.
pub const JOB_PATTERN: &[u8] = b"mailrs:outbound:job:*";

/// The pre-v2.5 row hash, field `blob`; cancelling a send deletes it.
pub fn legacy_row_key(id: impl std::fmt::Display) -> String {
    format!("mailrs:outbound:{id}")
}

/// The queue the sender pops from.
pub const PENDING_IDX: &[u8] = b"mailrs:outbound:pending-idx";

/// Where a future-dated send waits, scored by its send time.
///
/// The sender's due-sweep once had a second constant of its own —
/// `mailrs:outbound:scheduled`, without the suffix — and swept that
/// instead. Nothing has written that key since the queue moved to the
/// `-idx` names in v2.5.3, so a scheduled message was enqueued here,
/// never promoted, and never left.
pub const SCHEDULED_IDX: &[u8] = b"mailrs:outbound:scheduled-idx";

/// Scored by when the next attempt is due; the sender's due-sweep moves
/// members onto `pending-idx` once that time has passed.
///
/// Retries used to go straight back onto `pending-idx`. A job whose
/// retry floor had not passed was popped, pushed back and popped again,
/// and `pending-idx` grew duplicate ids that all read `state=pending`,
/// so none of them was ever filtered out. On 2026-10-16 it held 59,211
/// entries for 18 jobs to an address that cannot resolve, and new mail
/// waited about eight hours behind them. A zset keeps one entry per job
/// however often it is parked.
pub const RETRY_IDX: &[u8] = b"mailrs:outbound:retry-idx";

/// Ids of jobs that failed for good; the sender adds them.
pub const FAILED_SET: &[u8] = b"mailrs:outbound:failed";

/// The sender's record of why one job failed for good.
pub fn failed_audit_key(id: impl std::fmt::Display) -> String {
    format!("mailrs:outbound:failed:{id}")
}

/// The pre-v2.5 pending list; nothing pops it any more, and the sieve
/// redirect test asserts nothing is pushed there either.
pub const LEGACY_PENDING: &[u8] = b"mailrs:outbound:pending";

/// Ids claimed by the pre-v2.5 path and not yet settled.
pub const INFLIGHT: &[u8] = b"mailrs:outbound:inflight";

/// Recipients that bounced; nothing is sent to them again.
pub const SUPPRESSION: &[u8] = b"mailrs:outbound:suppression";

/// Next job id.
pub const COUNTER: &[u8] = b"mailrs:outbound:counter";

#[cfg(test)]
mod tests {
    use super::*;

    /// Prod queues live under these bytes; a changed spelling strands them.
    #[test]
    fn outbound_keys_keep_their_prod_spelling() {
        assert_eq!(job_key(7), "mailrs:outbound:job:7");
        assert_eq!(job_key("7"), "mailrs:outbound:job:7");
        assert!(job_key(7).starts_with(JOB_PREFIX));
        assert_eq!(JOB_PATTERN, b"mailrs:outbound:job:*");
        assert_eq!(legacy_row_key(7), "mailrs:outbound:7");
        assert_eq!(PENDING_IDX, b"mailrs:outbound:pending-idx");
        assert_eq!(SCHEDULED_IDX, b"mailrs:outbound:scheduled-idx");
        assert_eq!(RETRY_IDX, b"mailrs:outbound:retry-idx");
        assert_eq!(FAILED_SET, b"mailrs:outbound:failed");
        assert_eq!(failed_audit_key(7), "mailrs:outbound:failed:7");
        assert_eq!(LEGACY_PENDING, b"mailrs:outbound:pending");
        assert_eq!(INFLIGHT, b"mailrs:outbound:inflight");
        assert_eq!(SUPPRESSION, b"mailrs:outbound:suppression");
        assert_eq!(COUNTER, b"mailrs:outbound:counter");
    }
}
