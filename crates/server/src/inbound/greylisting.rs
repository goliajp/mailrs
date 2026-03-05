/// greylisting evaluation — pure logic, no database
#[derive(Debug, Clone)]
pub struct GreylistConfig {
    /// seconds to wait before accepting a retried message
    pub initial_delay_secs: u64,
    /// seconds to keep a passed triplet (auto-accept window)
    pub pass_ttl_secs: u64,
}

impl Default for GreylistConfig {
    fn default() -> Self {
        Self {
            initial_delay_secs: 300,       // 5 minutes
            pass_ttl_secs: 36 * 24 * 3600, // 36 days
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreylistDecision {
    /// first time seeing this triplet — defer
    Defer,
    /// seen before but too early to accept — defer
    TooEarly,
    /// delay has passed — accept and mark as passed
    Accept,
}

/// evaluate a (client_ip, sender, recipient) triplet
///
/// `first_seen`: unix timestamp when the triplet was first recorded (None if never seen)
/// `now`: current unix timestamp
pub fn evaluate_triplet(
    first_seen: Option<u64>,
    now: u64,
    config: &GreylistConfig,
) -> GreylistDecision {
    match first_seen {
        None => GreylistDecision::Defer,
        Some(seen) => {
            let elapsed = now.saturating_sub(seen);
            if elapsed < config.initial_delay_secs {
                GreylistDecision::TooEarly
            } else {
                GreylistDecision::Accept
            }
        }
    }
}

/// compute a triplet key for storage
pub fn triplet_key(client_ip: &str, sender: &str, recipient: &str) -> String {
    format!("{client_ip}|{sender}|{recipient}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GreylistConfig {
        GreylistConfig {
            initial_delay_secs: 300,
            pass_ttl_secs: 86400,
        }
    }

    #[test]
    fn first_time_defers() {
        let decision = evaluate_triplet(None, 1000, &config());
        assert_eq!(decision, GreylistDecision::Defer);
    }

    #[test]
    fn too_early_defers() {
        // first seen at t=1000, now t=1100 (100s < 300s delay)
        let decision = evaluate_triplet(Some(1000), 1100, &config());
        assert_eq!(decision, GreylistDecision::TooEarly);
    }

    #[test]
    fn after_delay_accepts() {
        // first seen at t=1000, now t=1400 (400s > 300s delay)
        let decision = evaluate_triplet(Some(1000), 1400, &config());
        assert_eq!(decision, GreylistDecision::Accept);
    }

    #[test]
    fn custom_delay_config() {
        let cfg = GreylistConfig {
            initial_delay_secs: 60,
            pass_ttl_secs: 3600,
        };
        // 59 seconds — too early
        assert_eq!(
            evaluate_triplet(Some(0), 59, &cfg),
            GreylistDecision::TooEarly
        );
        // 60 seconds — accept
        assert_eq!(
            evaluate_triplet(Some(0), 60, &cfg),
            GreylistDecision::Accept
        );
    }

    #[test]
    fn triplet_key_format() {
        let key = triplet_key("1.2.3.4", "sender@example.com", "rcpt@example.com");
        assert_eq!(key, "1.2.3.4|sender@example.com|rcpt@example.com");
    }

    // --- triplet_key additional tests ---

    #[test]
    fn triplet_key_ipv6() {
        let key = triplet_key("2001:db8::1", "a@b.com", "c@d.com");
        assert_eq!(key, "2001:db8::1|a@b.com|c@d.com");
    }

    #[test]
    fn triplet_key_empty_sender() {
        // bounce messages have empty sender (<>)
        let key = triplet_key("10.0.0.1", "", "postmaster@example.com");
        assert_eq!(key, "10.0.0.1||postmaster@example.com");
    }

    #[test]
    fn triplet_key_preserves_case() {
        let key = triplet_key("10.0.0.1", "User@Example.COM", "Admin@Test.ORG");
        assert_eq!(key, "10.0.0.1|User@Example.COM|Admin@Test.ORG");
    }

    #[test]
    fn triplet_key_special_chars() {
        let key = triplet_key("10.0.0.1", "user+tag@example.com", "o'malley@test.org");
        assert_eq!(key, "10.0.0.1|user+tag@example.com|o'malley@test.org");
    }

    // --- GreylistConfig defaults and boundary ---

    #[test]
    fn default_config_values() {
        let cfg = GreylistConfig::default();
        assert_eq!(cfg.initial_delay_secs, 300);
        assert_eq!(cfg.pass_ttl_secs, 36 * 24 * 3600);
    }

    #[test]
    fn zero_delay_accepts_immediately() {
        let cfg = GreylistConfig {
            initial_delay_secs: 0,
            pass_ttl_secs: 3600,
        };
        // even at the same timestamp it should accept
        assert_eq!(
            evaluate_triplet(Some(100), 100, &cfg),
            GreylistDecision::Accept
        );
    }

    #[test]
    fn very_large_delay() {
        let cfg = GreylistConfig {
            initial_delay_secs: u64::MAX,
            pass_ttl_secs: u64::MAX,
        };
        // no matter how far in the future, saturating_sub prevents overflow
        assert_eq!(
            evaluate_triplet(Some(0), u64::MAX - 1, &cfg),
            GreylistDecision::TooEarly
        );
    }

    // --- evaluate_triplet time window logic ---

    #[test]
    fn exact_boundary_accepts() {
        // exactly at the delay boundary should accept
        let cfg = config();
        assert_eq!(
            evaluate_triplet(Some(1000), 1300, &cfg),
            GreylistDecision::Accept
        );
    }

    #[test]
    fn one_second_before_boundary_defers() {
        let cfg = config();
        assert_eq!(
            evaluate_triplet(Some(1000), 1299, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn now_equals_first_seen_too_early() {
        let cfg = config();
        // elapsed = 0, less than 300
        assert_eq!(
            evaluate_triplet(Some(500), 500, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn now_before_first_seen_saturates_to_zero() {
        // clock skew scenario: now < first_seen
        let cfg = config();
        // saturating_sub(500, 1000) = 0, which is < 300
        assert_eq!(
            evaluate_triplet(Some(1000), 500, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn long_after_delay_still_accepts() {
        let cfg = config();
        // first seen a day ago
        assert_eq!(
            evaluate_triplet(Some(0), 86400, &cfg),
            GreylistDecision::Accept
        );
    }

    // --- GreylistDecision enum properties ---

    #[test]
    fn decision_clone_and_debug() {
        let d = GreylistDecision::Defer;
        let d2 = d.clone();
        assert_eq!(d, d2);
        // debug format should contain variant name
        assert!(format!("{d:?}").contains("Defer"));
    }

    #[test]
    fn decisions_are_distinct() {
        assert_ne!(GreylistDecision::Defer, GreylistDecision::TooEarly);
        assert_ne!(GreylistDecision::TooEarly, GreylistDecision::Accept);
        assert_ne!(GreylistDecision::Defer, GreylistDecision::Accept);
    }

    #[test]
    fn config_clone() {
        let cfg = GreylistConfig::default();
        let cfg2 = cfg.clone();
        assert_eq!(cfg.initial_delay_secs, cfg2.initial_delay_secs);
        assert_eq!(cfg.pass_ttl_secs, cfg2.pass_ttl_secs);
    }
}
