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
}
