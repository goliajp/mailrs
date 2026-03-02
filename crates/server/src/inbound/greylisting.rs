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
        assert_eq!(evaluate_triplet(Some(0), 59, &cfg), GreylistDecision::TooEarly);
        // 60 seconds — accept
        assert_eq!(evaluate_triplet(Some(0), 60, &cfg), GreylistDecision::Accept);
    }

    #[test]
    fn triplet_key_format() {
        let key = triplet_key("1.2.3.4", "sender@example.com", "rcpt@example.com");
        assert_eq!(key, "1.2.3.4|sender@example.com|rcpt@example.com");
    }
}
