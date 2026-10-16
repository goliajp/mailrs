//! Where a job waits between delivery attempts.

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
