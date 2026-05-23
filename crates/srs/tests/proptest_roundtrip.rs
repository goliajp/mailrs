//! Property-based roundtrip tests for SRS rewrite/reverse.
//!
//! Contract: for every valid sender address `user@domain`,
//! `reverse(rewrite(sender, local_domain, secret), secret, window_days)`
//! returns `Some(sender)` (modulo trailing-dot / case normalization).

use mailrs_srs::{reverse, rewrite};
use proptest::prelude::*;

/// Regex generators that match valid (simplified) address-local-part
/// and domain shapes — avoids generating addresses SRS itself would
/// reject, so we test the roundtrip not the input parser.
const LOCAL_PART: &str = "[a-z0-9][a-z0-9._-]{0,32}";
const DOMAIN: &str = "[a-z]{1,8}\\.[a-z]{2,4}";

proptest! {
    /// Rewrite + reverse recovers the original sender within the
    /// validity window. Uses a fixed secret + 7-day window — varying
    /// these is the job of regular unit tests, not the property.
    #[test]
    fn rewrite_then_reverse_recovers_sender(
        local in LOCAL_PART,
        original_domain in DOMAIN,
        local_domain in DOMAIN,
    ) {
        let sender = format!("{local}@{original_domain}");
        let rewritten = rewrite(&sender, &local_domain, "secret-key-for-test");
        // Sanity: rewritten address should be `srs0=...@local_domain`.
        prop_assert!(
            rewritten.ends_with(&format!("@{local_domain}")),
            "rewritten doesn't end with local_domain: {rewritten}"
        );

        let recovered = reverse(&rewritten, "secret-key-for-test", 7);
        prop_assert_eq!(recovered.as_deref(), Some(sender.as_str()));
    }

    /// Reverse with wrong secret yields None.
    #[test]
    fn wrong_secret_rejects(
        local in LOCAL_PART,
        original_domain in DOMAIN,
        local_domain in DOMAIN,
    ) {
        let sender = format!("{local}@{original_domain}");
        let rewritten = rewrite(&sender, &local_domain, "correct-secret");
        let recovered = reverse(&rewritten, "wrong-secret", 7);
        prop_assert!(recovered.is_none());
    }
}
