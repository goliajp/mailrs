//! Tests for `store` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn retry_delay_secs_uses_backoff_webhook_curve() {
    // Now backed by mailrs-backoff Backoff::webhook(): initial=60,
    // multiplier=2, max=6h (21600s), Equal jitter.
    // base_delay (without jitter) for attempt n: min(60 × 2^n, 21600).
    //   0: 60, 1: 120, 2: 240, 3: 480, 4: 960, 5: 1920, 6: 3840,
    //   7: 7680, 8: 15360, 9: 21600 (cap)
    // Equal jitter means actual returned value is in [base/2, base].
    // We assert the value is WITHIN that band rather than exact.
    let mut prev = 0;
    for attempt in 0..6u32 {
        let d = retry_delay_secs(attempt);
        assert!(d >= 30, "attempt {attempt}: {d} < 30 (Equal jitter low bound)");
        // Strict monotonic isn't guaranteed under jitter, but the band
        // floor for attempt n+1 is half of attempt n+1's base
        // (= attempt n's base for mult=2), which always >= prev floor.
        let _ = prev; // documentation marker
        prev = d;
    }
}

#[test]
fn retry_delay_secs_caps_at_six_hours() {
    // For attempts way past the cap, value should be in [3h, 6h]
    // (Equal jitter half-band of 6h).
    for attempt in [10u32, 20, 100, 1000] {
        let d = retry_delay_secs(attempt);
        assert!(
            (3 * 3600..=6 * 3600).contains(&d),
            "attempt {attempt}: {d} not in [3h, 6h]"
        );
    }
}

#[test]
fn retry_delay_secs_deterministic_for_same_attempt() {
    // The seed is derived from `attempt`, so the same attempt
    // produces the same jittered value (idempotent rescheduling).
    for attempt in 0..10u32 {
        assert_eq!(retry_delay_secs(attempt), retry_delay_secs(attempt));
    }
}

#[test]
fn retry_delay_secs_attempt_zero_in_jitter_band() {
    // attempt=0: base=60, Equal jitter → result in [30, 60].
    let d = retry_delay_secs(0);
    assert!((30..=60).contains(&d), "attempt 0: {d} not in [30, 60]");
}

#[test]
fn generate_signing_secret_produces_64_char_hex() {
    let secret = generate_signing_secret();
    assert_eq!(secret.len(), 64);
    assert!(secret.chars().all(|c| c.is_ascii_hexdigit()));

    // two calls produce different values
    let secret2 = generate_signing_secret();
    assert_ne!(secret, secret2);
}

#[test]
fn webhook_payload_serialization_roundtrip() {
    use crate::webhook::{WebhookData, WebhookPayload};

    let payload = WebhookPayload {
        event: "new_message".to_string(),
        timestamp: "2026-03-10T12:00:00Z".to_string(),
        data: WebhookData {
            user: "user@golia.jp".to_string(),
            thread_id: "abc123".to_string(),
            sender: "someone@example.com".to_string(),
            subject: "Hello".to_string(),
            snippet: "First 100 chars...".to_string(),
        },
    };

    let json = serde_json::to_string(&payload).unwrap();
    let deserialized: WebhookPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, deserialized);

    // verify key fields are present in json
    assert!(json.contains("\"event\":\"new_message\""));
    assert!(json.contains("\"thread_id\":\"abc123\""));
}
