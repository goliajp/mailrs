//! Greylisting policy + an optional Kevy-backed store.
//!
//! The policy is pure (no I/O): you call [`evaluate_triplet`](crate::greylist::evaluate_triplet) with the
//! first-seen timestamp and the current time, and it tells you whether
//! to defer, retry, or accept. The store is just there for convenience —
//! plug your own backend in by passing the right `first_seen` value.

/// Greylisting evaluation — pure logic, no database.
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

/// Outcome of evaluating a greylisting triplet — one of three states the
/// caller maps to an SMTP response code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GreylistDecision {
    /// first time seeing this triplet — defer
    Defer,
    /// seen before but too early to accept — defer
    TooEarly,
    /// delay has passed — accept and mark as passed
    Accept,
}

/// Evaluate a (client_ip, sender, recipient) triplet.
///
/// `first_seen`: unix timestamp when the triplet was first recorded (None if never seen).
/// `now`: current unix timestamp.
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

/// Compute a stable triplet key for storage.
///
/// Pre-sized + push-based to avoid the geometric resize cascade that
/// `format!` does (16 → 32 → 64 byte capacity steps). Capacity bound
/// is the three inputs + 2 separator bytes; typical real key is
/// ~50-80 bytes (IPv4 + two email addresses).
pub fn triplet_key(client_ip: &str, sender: &str, recipient: &str) -> String {
    let mut out = String::with_capacity(client_ip.len() + sender.len() + recipient.len() + 2);
    out.push_str(client_ip);
    out.push('|');
    out.push_str(sender);
    out.push('|');
    out.push_str(recipient);
    out
}

#[cfg(feature = "kevy-store")]
pub use kevy_impl::GreylistDb;

#[cfg(feature = "kevy-store")]
mod kevy_impl {
    use std::time::Duration;

    use kevy_embedded::Store;

    use super::{GreylistConfig, GreylistDecision, evaluate_triplet};

    /// Connection pool of the active SQL backend (PostgreSQL by
    /// default, spg-embedded behind the `spg` feature).
    #[cfg(not(feature = "spg"))]
    pub type BackendPool = sqlx::PgPool;
    /// Connection pool of the active SQL backend (PostgreSQL by
    /// default, spg-embedded behind the `spg` feature).
    #[cfg(feature = "spg")]
    pub type BackendPool = spg_sqlx::SpgPool;

    /// Kevy-backed greylisting store with an optional Postgres cold-backup.
    ///
    /// First-seen timestamps live in the in-process kevy [`Store`] with a TTL
    /// equal to `pass_ttl_secs`; the optional PG pool is written best-effort
    /// for durability across restarts.
    pub struct GreylistDb {
        kevy: Store,
        pg: Option<BackendPool>,
    }

    impl GreylistDb {
        /// Construct a kevy-only greylisting store from an in-process
        /// [`kevy_embedded::Store`] handle. `Store` is `Clone`, so callers
        /// typically pass a clone of the shared cement-owned store.
        pub fn new(kevy: Store) -> Self {
            Self { kevy, pg: None }
        }

        /// Attach a Postgres pool for best-effort durability across kevy
        /// restarts (writes are best-effort; failures don't propagate).
        pub fn with_pg(mut self, pool: BackendPool) -> Self {
            self.pg = Some(pool);
            self
        }

        /// Look up the triplet `key`, update its first-seen entry as needed,
        /// and return the resulting [`GreylistDecision`].
        ///
        /// Read order: kevy (hot, sub-ms) → PG (cold backup, ~5 ms RTT).
        /// On kevy miss with a PG pool attached we cold-load the
        /// `first_seen` value from PG and warm the kevy entry, so a single
        /// PG read suffices to bring a triplet back to hot. This is the
        /// post-INC-2026-06-03 warmup path: when kevy AOF was reset every
        /// genuine known sender was hitting "first seen → Defer" and the
        /// system was 451-deferring everyone for ~5 minutes. With cold
        /// load any sender ever recorded in `greylist_triplets` is
        /// indistinguishable from a hot-cached one after one query.
        pub async fn check(
            &self,
            key: &str,
            now: u64,
            config: &GreylistConfig,
        ) -> GreylistDecision {
            let vk_key = format!("gl:{key}");
            let ttl = Duration::from_secs(config.pass_ttl_secs);

            // 1) hot path — read from kevy
            let mut first_seen: Option<u64> = self
                .kevy
                .get(vk_key.as_bytes())
                .ok()
                .flatten()
                .and_then(|bytes| std::str::from_utf8(&bytes).ok()?.parse().ok());

            // 2) kevy miss + PG configured → cold-load from PG and warm kevy.
            //    This is best-effort: PG read failures fall through to
            //    `first_seen = None` (the genuine new-sender path), which
            //    matches pre-warmup behaviour — never worse than before.
            if first_seen.is_none()
                && let Some(ref pool) = self.pg
            {
                let row: Option<(i64, i64)> = sqlx::query_as(
                    "SELECT first_seen, last_seen FROM greylist_triplets WHERE triplet = $1",
                )
                .bind(key)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
                if let Some((fs, _ls)) = row {
                    let fs_u64 = fs.max(0) as u64;
                    first_seen = Some(fs_u64);
                    // warm kevy so subsequent checks skip the PG round-trip.
                    // value is the historical first_seen, not `now`, so the
                    // delay window is computed correctly even right after
                    // warmup.
                    let warm_value = fs_u64.to_string();
                    let _ = self
                        .kevy
                        .set_with_ttl(vk_key.as_bytes(), warm_value.as_bytes(), ttl);
                }
            }

            let decision = evaluate_triplet(first_seen, now, config);

            match decision {
                GreylistDecision::Defer => {
                    // first time anywhere — set with TTL = pass_ttl
                    let value = now.to_string();
                    let _ = self
                        .kevy
                        .set_with_ttl(vk_key.as_bytes(), value.as_bytes(), ttl);
                }
                GreylistDecision::TooEarly | GreylistDecision::Accept => {
                    // refresh TTL to keep entry alive
                    let _ = self.kevy.expire(vk_key.as_bytes(), ttl);
                }
            }

            // cold backup to PG (best effort)
            if let Some(ref pool) = self.pg {
                let now_i64 = now as i64;
                let _ = sqlx::query(
                    "INSERT INTO greylist_triplets (triplet, first_seen, last_seen)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (triplet) DO UPDATE SET last_seen = $3",
                )
                .bind(key)
                .bind(first_seen.unwrap_or(now) as i64)
                .bind(now_i64)
                .execute(pool)
                .await;
            }

            decision
        }
    }
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
        let decision = evaluate_triplet(Some(1000), 1100, &config());
        assert_eq!(decision, GreylistDecision::TooEarly);
    }

    #[test]
    fn after_delay_accepts() {
        let decision = evaluate_triplet(Some(1000), 1400, &config());
        assert_eq!(decision, GreylistDecision::Accept);
    }

    #[test]
    fn custom_delay_config() {
        let cfg = GreylistConfig {
            initial_delay_secs: 60,
            pass_ttl_secs: 3600,
        };
        assert_eq!(
            evaluate_triplet(Some(0), 59, &cfg),
            GreylistDecision::TooEarly
        );
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

    #[test]
    fn triplet_key_ipv6() {
        let key = triplet_key("2001:db8::1", "a@b.com", "c@d.com");
        assert_eq!(key, "2001:db8::1|a@b.com|c@d.com");
    }

    #[test]
    fn triplet_key_empty_sender() {
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
        assert_eq!(
            evaluate_triplet(Some(0), u64::MAX - 1, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn exact_boundary_accepts() {
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
        assert_eq!(
            evaluate_triplet(Some(500), 500, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn now_before_first_seen_saturates_to_zero() {
        let cfg = config();
        assert_eq!(
            evaluate_triplet(Some(1000), 500, &cfg),
            GreylistDecision::TooEarly
        );
    }

    #[test]
    fn long_after_delay_still_accepts() {
        let cfg = config();
        assert_eq!(
            evaluate_triplet(Some(0), 86400, &cfg),
            GreylistDecision::Accept
        );
    }

    #[test]
    fn decision_clone_and_debug() {
        let d = GreylistDecision::Defer;
        let d2 = d.clone();
        assert_eq!(d, d2);
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
