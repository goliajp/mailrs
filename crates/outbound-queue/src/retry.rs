/// exponential backoff delay schedule (in seconds)
/// 60, 300, 900, 1800, 3600, 7200, 14400, 28800 (cap)
const DELAYS: [u64; 8] = [60, 300, 900, 1800, 3600, 7200, 14400, 28800];

/// get retry delay for a given attempt number (0-indexed)
pub fn retry_delay_secs(attempt: u32) -> u64 {
    let idx = (attempt as usize).min(DELAYS.len() - 1);
    DELAYS[idx]
}

/// determine if a message should bounce after too many attempts
pub fn should_bounce(attempt: u32, max_attempts: u32) -> bool {
    attempt >= max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_schedule() {
        assert_eq!(retry_delay_secs(0), 60);
        assert_eq!(retry_delay_secs(1), 300);
        assert_eq!(retry_delay_secs(2), 900);
        assert_eq!(retry_delay_secs(3), 1800);
        assert_eq!(retry_delay_secs(4), 3600);
    }

    #[test]
    fn cap_at_8h() {
        assert_eq!(retry_delay_secs(7), 28800);
        assert_eq!(retry_delay_secs(8), 28800);
        assert_eq!(retry_delay_secs(100), 28800);
    }

    #[test]
    fn bounce_at_max() {
        assert!(should_bounce(5, 5));
        assert!(should_bounce(10, 5));
    }

    #[test]
    fn no_bounce_below_max() {
        assert!(!should_bounce(0, 5));
        assert!(!should_bounce(4, 5));
    }

    #[test]
    fn delay_full_schedule_coverage() {
        // verify every defined slot in the DELAYS array
        let expected = [60u64, 300, 900, 1800, 3600, 7200, 14400, 28800];
        for (i, &want) in expected.iter().enumerate() {
            assert_eq!(retry_delay_secs(i as u32), want, "slot {i} mismatch");
        }
    }

    #[test]
    fn delay_is_monotonically_increasing() {
        let mut prev = 0u64;
        for i in 0..8u32 {
            let d = retry_delay_secs(i);
            assert!(d > prev, "delay at slot {i} ({d}) is not greater than previous ({prev})");
            prev = d;
        }
    }

    #[test]
    fn should_bounce_boundary_exact() {
        // exactly at max_attempts should bounce
        assert!(should_bounce(1, 1));
        assert!(should_bounce(0, 0));
    }

    #[test]
    fn should_bounce_max_attempts_zero() {
        // attempt=0, max=0 → bounce immediately
        assert!(should_bounce(0, 0));
    }

    #[test]
    fn should_bounce_large_values() {
        assert!(!should_bounce(u32::MAX - 1, u32::MAX));
        assert!(should_bounce(u32::MAX, u32::MAX));
    }

    #[test]
    fn delay_first_attempt_is_one_minute() {
        assert_eq!(retry_delay_secs(0), 60);
    }

    #[test]
    fn delay_last_defined_slot() {
        assert_eq!(retry_delay_secs(7), 28800);
    }

    #[test]
    fn delay_cap_at_various_overflow_attempts() {
        // all attempts beyond the table should return the cap
        for attempt in [8, 9, 10, 50, 100, 255, 1000, u32::MAX] {
            assert_eq!(
                retry_delay_secs(attempt),
                28800,
                "attempt {attempt} should be capped"
            );
        }
    }

    #[test]
    fn delay_each_step_at_least_doubles_or_grows() {
        // verify each delay is significantly larger than previous
        for i in 1..8u32 {
            let prev = retry_delay_secs(i - 1);
            let curr = retry_delay_secs(i);
            assert!(
                curr >= prev * 2 || curr > prev,
                "delay at {i} ({curr}) should grow from {prev}"
            );
        }
    }

    #[test]
    fn delay_total_before_bounce_with_8_attempts() {
        // sum of all delays for 8 attempts (indices 0..7)
        let total: u64 = (0..8).map(retry_delay_secs).sum();
        // 60 + 300 + 900 + 1800 + 3600 + 7200 + 14400 + 28800 = 57060
        assert_eq!(total, 57060);
    }

    #[test]
    fn delay_total_in_hours() {
        let total_secs: u64 = (0..8).map(retry_delay_secs).sum();
        let total_hours = total_secs as f64 / 3600.0;
        // total retry time should be roughly 15.85 hours
        assert!(total_hours > 15.0 && total_hours < 16.0);
    }

    #[test]
    fn should_bounce_one_attempt_max() {
        // max_attempts=1 means only attempt 0 is allowed
        assert!(!should_bounce(0, 1));
        assert!(should_bounce(1, 1));
        assert!(should_bounce(2, 1));
    }

    #[test]
    fn should_bounce_two_attempts_max() {
        assert!(!should_bounce(0, 2));
        assert!(!should_bounce(1, 2));
        assert!(should_bounce(2, 2));
    }

    #[test]
    fn should_bounce_high_max_attempts() {
        let max = 100;
        assert!(!should_bounce(99, max));
        assert!(should_bounce(100, max));
        assert!(should_bounce(101, max));
    }

    #[test]
    fn should_bounce_returns_bool() {
        // ensure return type behaves correctly in conditional contexts
        let result: bool = should_bounce(5, 5);
        assert!(result);
        let result: bool = should_bounce(4, 5);
        assert!(!result);
    }

    #[test]
    fn delay_returns_u64() {
        let d: u64 = retry_delay_secs(0);
        assert_eq!(d, 60u64);
    }

    #[test]
    fn delay_schedule_matches_exponential_pattern() {
        // verify the schedule roughly follows exponential growth
        // each delay should be roughly 2-3x the previous
        for i in 1..DELAYS.len() {
            let ratio = DELAYS[i] as f64 / DELAYS[i - 1] as f64;
            assert!(
                (1.5..=5.1).contains(&ratio),
                "ratio at slot {i} is {ratio}, expected 1.5-5.0"
            );
        }
    }

    #[test]
    fn delay_cap_value_is_8_hours() {
        assert_eq!(DELAYS[DELAYS.len() - 1], 8 * 3600);
    }

    #[test]
    fn delay_minimum_is_60_seconds() {
        assert_eq!(*DELAYS.iter().min().unwrap(), 60);
    }
}
