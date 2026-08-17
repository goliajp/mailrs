//! When to try again, and when to stop trying.
//!
//! RFC 5321 §4.5.4.1: *"The sender MUST delay retrying a particular
//! destination after one attempt has failed … the give-up time
//! generally needs to be at least 4-5 days."* Every general-purpose MTA
//! does this — Postfix pairs `minimal_backoff_time` / `maximal_
//! backoff_time` with `maximal_queue_lifetime`, and Exim the same shape.
//!
//! This sender did neither. It retried on a flat 60-second floor and
//! gave up after 10 attempts, so a message's whole life was **ten
//! minutes**, and the recipient got a permanent bounce for a condition
//! that had not been given time to be temporary.
//!
//! 2026-08-17 is what that costs. Microsoft had our egress IP on a
//! reputation block; the mail bounced at 00:46, the delisting was
//! granted that afternoon, and their own notice says it takes 24-48
//! hours to propagate. With a normal queue lifetime the message would
//! have sat there and delivered itself when the block lifted, and
//! nobody would have had to do anything. Instead it was gone by 00:47.
//!
//! Two changes, both of them the ordinary thing:
//!
//! * **Exponential backoff**, from `mailrs_backoff::Backoff::
//!   smtp_outbound` — a preset written for this and, until now, used
//!   only by the dormant lane's queue while production ran the flat
//!   floor.
//! * **Give up on age, not on attempt count.** How many attempts fit in
//!   five days depends entirely on the schedule; "five days" does not.
//!   The attempt cap stays as a backstop against a pathological loop,
//!   which is all a count was ever good for.

use mailrs_backoff::Backoff;

/// Give-up age, in seconds. Postfix's `maximal_queue_lifetime` default
/// is 5 days and RFC 5321 asks for 4-5; this is the same number.
pub(super) const DEFAULT_MAX_QUEUE_LIFETIME_SECS: i64 = 5 * 24 * 3600;

/// Attempt backstop. Not the real bound — the age is. With the 8-hour
/// cap, five days is about 22 attempts, so this only fires if something
/// is retrying far faster than the schedule says, which would be a bug
/// rather than a slow remote.
pub(super) const DEFAULT_MAX_ATTEMPTS: u32 = 100;

/// How long to wait before attempt `attempt + 1`, given that `attempt`
/// have already been made.
///
/// `seed` makes the jitter deterministic per message: two messages to
/// the same dead host must not synchronise, and the same message must
/// pick the same delay if this is evaluated twice.
pub(super) fn retry_floor_secs(attempts_so_far: u32, seed: u64) -> i64 {
    Backoff::smtp_outbound()
        .delay(attempts_so_far, seed)
        .as_secs() as i64
}

/// Whether to stop retrying and bounce.
///
/// Age first, because that is the rule; the attempt cap is the
/// backstop.
pub(super) fn give_up(
    age_secs: i64,
    attempts: u32,
    max_lifetime_secs: i64,
    max_attempts: u32,
) -> bool {
    age_secs >= max_lifetime_secs || attempts >= max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A message gets days, not minutes.**
    ///
    /// The bound is its age. Ten attempts on a flat minute made a
    /// message's whole life ten minutes, which is shorter than the
    /// propagation delay of the very unblock that would have delivered
    /// it.
    #[test]
    fn a_message_lives_for_days() {
        let five_days = DEFAULT_MAX_QUEUE_LIFETIME_SECS;
        assert!(
            five_days >= 4 * 24 * 3600,
            "RFC 5321 asks for at least 4-5 days"
        );

        // An hour old, a few attempts in: nowhere near giving up.
        assert!(!give_up(3600, 5, five_days, DEFAULT_MAX_ATTEMPTS));
        // Four days and plenty of attempts: still trying.
        assert!(!give_up(4 * 24 * 3600, 20, five_days, DEFAULT_MAX_ATTEMPTS));
        // Past the lifetime: done.
        assert!(give_up(five_days, 20, five_days, DEFAULT_MAX_ATTEMPTS));
    }

    /// The attempt cap is a backstop against a loop, not the schedule.
    /// It must not be reachable inside the lifetime by a well-behaved
    /// retry sequence.
    #[test]
    fn the_attempt_cap_does_not_bind_before_the_age_does() {
        let mut age = 0i64;
        let mut attempts = 0u32;
        while !give_up(
            age,
            attempts,
            DEFAULT_MAX_QUEUE_LIFETIME_SECS,
            DEFAULT_MAX_ATTEMPTS,
        ) {
            age += retry_floor_secs(attempts, 7);
            attempts += 1;
        }
        assert!(
            age >= DEFAULT_MAX_QUEUE_LIFETIME_SECS,
            "the attempt cap fired first, after {attempts} attempts and {age}s — \
             the schedule, not the age, decided when to give up"
        );
        assert!(
            attempts < DEFAULT_MAX_ATTEMPTS,
            "{attempts} attempts to cover five days; the cap is {DEFAULT_MAX_ATTEMPTS}"
        );
    }

    /// **Backoff, not a flat floor.** A remote that is down stops being
    /// hammered once a minute for as long as we keep trying.
    #[test]
    fn the_wait_grows_with_the_attempt() {
        let seed = 42;
        let early = retry_floor_secs(1, seed);
        let mid = retry_floor_secs(5, seed);
        let late = retry_floor_secs(12, seed);
        assert!(
            early < mid && mid <= late,
            "delays did not grow: {early}s, {mid}s, {late}s"
        );
        // And it is capped rather than growing without bound — an
        // 8-hour ceiling, so a five-day queue still gets ~15 late tries.
        assert!(late <= 8 * 3600, "the cap is not holding: {late}s");
    }

    /// Two messages to the same dead host must not retry in lockstep,
    /// or a big queue turns every backoff step into a thundering herd.
    #[test]
    fn different_messages_do_not_synchronise() {
        let a: Vec<i64> = (1..8).map(|n| retry_floor_secs(n, 1)).collect();
        let b: Vec<i64> = (1..8).map(|n| retry_floor_secs(n, 999)).collect();
        assert_ne!(a, b, "jitter is not seeded per message: {a:?}");
    }

    /// And the same message evaluated twice picks the same delay —
    /// the floor is compared against a stored timestamp, so a delay
    /// that changed on each read would let a message through early.
    #[test]
    fn one_message_gets_one_answer() {
        assert_eq!(retry_floor_secs(4, 12345), retry_floor_secs(4, 12345));
    }
}
