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
pub use store_impl::EmbeddedGreylistBackend;
#[cfg(feature = "store")]
pub use store_impl::{GreylistBackend, GreylistDb, check_greylist_backend};

#[cfg(feature = "store")]
mod store_impl {
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;

    use super::{GreylistConfig, GreylistDecision, evaluate_triplet};

    /// The KV operations greylisting needs from its first-seen store.
    ///
    /// Abstracted so the same policy runs over the in-process kevy
    /// `Store` ([`EmbeddedGreylistBackend`], feature `kevy-store`) or a
    /// shared network kevy-server, whose impl lives with the caller so this
    /// crate needs no network client. Errors are folded into the fail-open
    /// shape the policy already assumes: a read failure reads as "not
    /// seen", a write failure is dropped.
    #[async_trait]
    pub trait GreylistBackend: Send + Sync {
        /// Read the first-seen value for `key`; `None` on miss or error.
        async fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
        /// Set `key` = `value` with a `ttl`; best-effort, errors dropped.
        async fn set_with_ttl(&self, key: &[u8], value: &[u8], ttl: Duration);
        /// Refresh `key`'s TTL; best-effort, errors dropped.
        async fn expire(&self, key: &[u8], ttl: Duration);
    }

    /// In-process [`GreylistBackend`] over a [`kevy_embedded::Store`].
    /// `Store` is `Clone` and its ops are synchronous sub-µs calls, so
    /// the async methods just delegate and resolve immediately.
    #[cfg(feature = "kevy-store")]
    pub struct EmbeddedGreylistBackend {
        kevy: kevy_embedded::Store,
    }

    #[cfg(feature = "kevy-store")]
    impl EmbeddedGreylistBackend {
        /// Wrap an in-process store; `Store` is `Clone`, so pass a clone of
        /// the shared one.
        pub fn new(kevy: kevy_embedded::Store) -> Self {
            Self { kevy }
        }
    }

    #[cfg(feature = "kevy-store")]
    #[async_trait]
    impl GreylistBackend for EmbeddedGreylistBackend {
        async fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
            self.kevy.get(key).ok().flatten()
        }
        async fn set_with_ttl(&self, key: &[u8], value: &[u8], ttl: Duration) {
            let _ = self.kevy.set_with_ttl(key, value, ttl);
        }
        async fn expire(&self, key: &[u8], ttl: Duration) {
            let _ = self.kevy.expire(key, ttl);
        }
    }

    /// Greylisting store over a [`GreylistBackend`].
    ///
    /// First-seen timestamps live in the backend with a TTL equal to
    /// `pass_ttl_secs`. Durability is the backend's: one that loses its
    /// data defers every known sender once more.
    pub struct GreylistDb {
        backend: Arc<dyn GreylistBackend>,
    }

    impl GreylistDb {
        /// Construct a kevy-only greylisting store from an in-process
        /// [`kevy_embedded::Store`] handle. `Store` is `Clone`, so callers
        /// typically pass a clone of the shared store.
        #[cfg(feature = "kevy-store")]
        pub fn new(kevy: kevy_embedded::Store) -> Self {
            Self::with_backend(Arc::new(EmbeddedGreylistBackend::new(kevy)))
        }

        /// Construct over an arbitrary [`GreylistBackend`], such as a
        /// shared network kevy-server.
        pub fn with_backend(backend: Arc<dyn GreylistBackend>) -> Self {
            Self { backend }
        }

        /// Look up the triplet `key`, update its first-seen entry as needed,
        /// and return the resulting [`GreylistDecision`].
        pub async fn check(
            &self,
            key: &str,
            now: u64,
            config: &GreylistConfig,
        ) -> GreylistDecision {
            let vk_key = crate::greylist_key::store_key(key);
            let ttl = Duration::from_secs(config.pass_ttl_secs);

            let first_seen: Option<u64> = self
                .backend
                .get(vk_key.as_bytes())
                .await
                .and_then(|bytes| std::str::from_utf8(&bytes).ok()?.parse().ok());
            let decision = evaluate_triplet(first_seen, now, config);

            match decision {
                GreylistDecision::Defer => {
                    // first time anywhere — set with TTL = pass_ttl
                    let value = now.to_string();
                    self.backend
                        .set_with_ttl(vk_key.as_bytes(), value.as_bytes(), ttl)
                        .await;
                }
                GreylistDecision::TooEarly | GreylistDecision::Accept => {
                    // refresh TTL to keep entry alive
                    self.backend.expire(vk_key.as_bytes(), ttl).await;
                }
            }

            decision
        }
    }

    /// Check a [`GreylistBackend`] against the rules [`GreylistDb`] relies
    /// on; panics naming the first one broken. Uses keys under
    /// `greylist-contract:`, which must not exist beforehand. Expiry
    /// timing is not checked: it would make the check as slow as the TTL.
    pub async fn check_greylist_backend(backend: &dyn GreylistBackend) {
        let ttl = Duration::from_secs(3600);
        let key = b"greylist-contract:a";
        assert_eq!(
            backend.get(key).await,
            None,
            "get: a missing key reads as None"
        );
        backend.set_with_ttl(key, b"1000", ttl).await;
        assert_eq!(
            backend.get(key).await.as_deref(),
            Some(&b"1000"[..]),
            "set_with_ttl: stored"
        );
        backend.set_with_ttl(key, b"2000", ttl).await;
        assert_eq!(
            backend.get(key).await.as_deref(),
            Some(&b"2000"[..]),
            "set_with_ttl: overwrites"
        );
        backend.expire(key, ttl).await;
        assert_eq!(
            backend.get(key).await.as_deref(),
            Some(&b"2000"[..]),
            "expire: keeps the value"
        );
        backend.expire(b"greylist-contract:missing", ttl).await;
        assert_eq!(
            backend.get(b"greylist-contract:missing").await,
            None,
            "expire: does not create a missing key"
        );
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

    // GreylistDb::check over the default in-process backend — proves the
    // GreylistBackend trait seam preserves the Defer → TooEarly → Accept
    // state machine end-to-end (not just the pure evaluate_triplet logic).
    #[cfg(feature = "kevy-store")]
    #[tokio::test]
    async fn the_in_process_backend_meets_the_contract() {
        let store = kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap();
        check_greylist_backend(&EmbeddedGreylistBackend::new(store)).await;
    }

    #[cfg(feature = "kevy-store")]
    #[tokio::test]
    async fn check_state_machine_over_in_process_backend() {
        use kevy_embedded::{Config, Store};

        let store = Store::open(Config::default()).expect("open in-memory kevy");
        let db = super::GreylistDb::new(store);
        let cfg = GreylistConfig::default(); // initial_delay 300s
        let key = "1.2.3.4|s@a.com|r@b.com";
        let t0 = 1_000_000u64;

        // first sighting → Defer (and records first_seen)
        assert_eq!(db.check(key, t0, &cfg).await, GreylistDecision::Defer);
        // retry within the delay window → TooEarly
        assert_eq!(
            db.check(key, t0 + 100, &cfg).await,
            GreylistDecision::TooEarly
        );
        // retry past the delay → Accept
        assert_eq!(
            db.check(key, t0 + cfg.initial_delay_secs, &cfg).await,
            GreylistDecision::Accept
        );
    }
}
