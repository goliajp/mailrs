//! How long a polling loop waits when it keeps finding nothing.
//!
//! Every periodic worker needs a resting state that costs almost nothing,
//! or an empty system pays for work it cannot use: the maildir self-heal
//! read 48,613 files every 31 seconds and reported repairs it had not made,
//! on a host shared with other services (2026-07-19). The rule that came out
//! of it is `.claude/rules/periodic-work-must-converge.md`.
//!
//! Two loops in this crate had the same doubling written out inline and
//! neither had a test. One definition means the shape is stated once and
//! pinned once.

use std::time::Duration;

/// Wait before the next round.
///
/// `idle_rounds` is how many consecutive rounds have accomplished nothing —
/// zero means the last round did something, and the loop stays responsive at
/// `busy`. Each idle round doubles the wait until it reaches `idle`.
///
/// The doubling is capped at 2^5 before `idle` clamps it, so the shift can
/// never approach the width of the type however long a loop stays quiet.
pub(crate) fn idle_backoff(busy: Duration, idle: Duration, idle_rounds: u32) -> Duration {
    match idle_rounds {
        0 => busy,
        n => busy.saturating_mul(1u32 << n.min(5)).min(idle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUSY: Duration = Duration::from_secs(5);
    const IDLE: Duration = Duration::from_secs(120);

    /// A loop that just did something is polled promptly again.
    #[test]
    fn a_productive_round_is_followed_by_the_busy_interval() {
        assert_eq!(idle_backoff(BUSY, IDLE, 0), BUSY);
    }

    #[test]
    fn each_idle_round_doubles_the_wait() {
        assert_eq!(idle_backoff(BUSY, IDLE, 1), Duration::from_secs(10));
        assert_eq!(idle_backoff(BUSY, IDLE, 2), Duration::from_secs(20));
        assert_eq!(idle_backoff(BUSY, IDLE, 3), Duration::from_secs(40));
    }

    /// The point of the whole thing: a loop quiet for a long time must not
    /// keep costing what a busy one costs.
    #[test]
    fn the_wait_settles_at_the_idle_interval() {
        assert_eq!(idle_backoff(BUSY, IDLE, 5), IDLE);
        assert_eq!(idle_backoff(BUSY, IDLE, 50), IDLE);
        assert_eq!(idle_backoff(BUSY, IDLE, u32::MAX), IDLE);
    }

    /// `1u32 << n` is undefined for n >= 32 and `saturating_mul` cannot save
    /// a shift that already wrapped, so the cap on the exponent is load
    /// bearing rather than cosmetic.
    #[test]
    fn a_loop_quiet_for_a_very_long_time_does_not_overflow_the_shift() {
        for rounds in [31u32, 32, 33, 64, 1_000_000, u32::MAX] {
            assert_eq!(idle_backoff(BUSY, IDLE, rounds), IDLE);
        }
    }

    /// Callers pass their own pair; the helper must not assume one loop's.
    #[test]
    fn the_intervals_come_from_the_caller() {
        let busy = Duration::from_secs(60);
        let idle = Duration::from_secs(120);
        assert_eq!(idle_backoff(busy, idle, 0), busy);
        assert_eq!(idle_backoff(busy, idle, 1), idle);
    }
}
