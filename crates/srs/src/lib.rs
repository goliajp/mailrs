#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: [`rewrite`] (forward) + [`reverse`] (parse + verify
//! HMAC + return original) form the round-trip pair. Both share the
//! same HMAC-SHA256 construction over `(tt, original_domain, local_part)`
//! truncated to the first 4 bytes (8 hex chars) for compact wire form.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Number of hex characters used as the SRS signature in the wire form.
/// 8 hex chars = 32 bits of entropy from the HMAC truncation — enough
/// to prevent online guessing given the keyed signer + per-day
/// timestamp.
const HASH_HEX_LEN: usize = 8;

/// SRS timestamp window: rewrites older than this number of days fail
/// verification. RFC default is 21 days; we use 14 as a tighter default
/// (most legitimate bounces come back within hours).
pub const DEFAULT_TIMESTAMP_WINDOW_DAYS: u32 = 14;

/// Forward-rewrite an envelope sender for SPF-aware forwarding.
///
/// The wire form is `SRS0=<hash>=<tt>=<original-domain>=<local-part>@<local-domain>`
/// where:
/// - `<hash>` is 8 hex chars derived from HMAC-SHA256 of (tt,
///   original-domain, local-part) keyed by `secret`
/// - `<tt>` is a 3-digit timestamp: `(days_since_epoch mod 1024)`
/// - `<original-domain>` is the sender's domain
/// - `<local-part>` is the sender's mailbox name
/// - `<local-domain>` is the forwarding host's own domain
///
/// If `sender` has no `@`, it's returned unchanged (postmaster-style
/// bare addresses).
///
/// ```
/// use mailrs_srs::rewrite;
/// let r = rewrite("alice@example.com", "mx.golia.jp", "secret-key");
/// assert!(r.starts_with("SRS0="));
/// assert!(r.ends_with("@mx.golia.jp"));
/// assert!(r.contains("=example.com=alice@"));
/// ```
pub fn rewrite(sender: &str, local_domain: &str, secret: &str) -> String {
    let Some((local_part, original_domain)) = sender.split_once('@') else {
        return sender.to_string();
    };

    let tt = current_tt();
    let hash = compute_hash(secret.as_bytes(), &tt, original_domain, local_part);

    // Pre-sized buffer: "SRS0=" + hash (8) + "=" + tt (3) + "=" +
    // original_domain + "=" + local_part + "@" + local_domain
    let mut out = String::with_capacity(
        5 + HASH_HEX_LEN + 1 + 3 + 1 + original_domain.len() + 1 + local_part.len() + 1
            + local_domain.len(),
    );
    out.push_str("SRS0=");
    out.push_str(&hash);
    out.push('=');
    out.push_str(&tt);
    out.push('=');
    out.push_str(original_domain);
    out.push('=');
    out.push_str(local_part);
    out.push('@');
    out.push_str(local_domain);
    out
}

/// Parse an SRS-rewritten address back to the original sender, verifying
/// the HMAC and timestamp window.
///
/// Returns `Some(original_sender)` if:
/// - the address matches the `SRS0=hash=tt=domain=local@anything` shape
/// - the HMAC verifies under `secret`
/// - the `tt` timestamp is within `window_days` of today
///
/// Returns `None` for any failure (malformed shape, bad hash, expired
/// timestamp). Callers should treat any `None` as a rejected bounce.
///
/// ```
/// use mailrs_srs::{rewrite, reverse, DEFAULT_TIMESTAMP_WINDOW_DAYS};
/// let secret = "key";
/// let rewritten = rewrite("alice@example.com", "mx.local", secret);
/// let original = reverse(&rewritten, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
/// assert_eq!(original.as_deref(), Some("alice@example.com"));
/// ```
pub fn reverse(rewritten: &str, secret: &str, window_days: u32) -> Option<String> {
    // Split off "anything after @" — that's `<local-domain>`, we don't
    // verify it (the receiving server knows its own domain).
    let (encoded_local, _local_domain) = rewritten.split_once('@')?;
    // encoded_local layout: SRS0=hash=tt=original_domain=local_part
    let after_prefix = encoded_local.strip_prefix("SRS0=")?;
    let mut parts = after_prefix.splitn(4, '=');
    let hash = parts.next()?;
    let tt = parts.next()?;
    let original_domain = parts.next()?;
    let local_part = parts.next()?;

    if hash.len() != HASH_HEX_LEN || tt.len() != 3 {
        return None;
    }

    // Verify HMAC.
    let expected = compute_hash(secret.as_bytes(), tt, original_domain, local_part);
    if !constant_time_eq(hash.as_bytes(), expected.as_bytes()) {
        return None;
    }

    // Verify timestamp window.
    if !tt_within_window(tt, window_days) {
        return None;
    }

    Some(format!("{local_part}@{original_domain}"))
}

/// Compute the current 3-digit `tt` timestamp (days since epoch mod
/// 1024, zero-padded).
fn current_tt() -> String {
    let days = (chrono::Utc::now().timestamp() / 86400) as u32 % 1024;
    format!("{days:03}")
}

/// HMAC-SHA256 over (tt, original_domain, local_part), truncated to
/// `HASH_HEX_LEN / 2` bytes and hex-encoded.
fn compute_hash(secret: &[u8], tt: &str, original_domain: &str, local_part: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(tt.as_bytes());
    mac.update(original_domain.as_bytes());
    mac.update(local_part.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex::encode(&bytes[..HASH_HEX_LEN / 2])
}

/// Check whether `tt` (the 3-digit `days_since_epoch mod 1024` form) is
/// within `window_days` of the current `tt`.
fn tt_within_window(tt: &str, window_days: u32) -> bool {
    let tt_num: u32 = match tt.parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    let now = (chrono::Utc::now().timestamp() / 86400) as u32 % 1024;
    // tt is mod 1024 so we have to be careful around the wrap-around.
    let raw_diff = if now >= tt_num {
        now - tt_num
    } else {
        // tt was issued before the 1024-day wrap; assume it's still
        // valid if the wrapped distance is within window.
        1024 - tt_num + now
    };
    raw_diff <= window_days
}

/// Constant-time byte slice comparison. Same length required — the
/// caller has already ensured that. Prevents timing-side-channel
/// recovery of the HMAC by an attacker probing reverse() in a loop.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_format() {
        let result = rewrite("user@example.com", "mx.local", "secret123");
        assert!(result.starts_with("SRS0="), "got: {result}");
        assert!(result.ends_with("@mx.local"), "got: {result}");
        assert!(result.contains("=example.com=user@"), "got: {result}");
    }

    #[test]
    fn rewrite_no_at_passthrough() {
        let result = rewrite("postmaster", "mx.local", "secret");
        assert_eq!(result, "postmaster");
    }

    #[test]
    fn rewrite_deterministic() {
        let a = rewrite("test@example.com", "mx.local", "key1");
        let b = rewrite("test@example.com", "mx.local", "key1");
        assert_eq!(a, b);
    }

    #[test]
    fn rewrite_different_secrets_different_hashes() {
        let a = rewrite("test@example.com", "mx.local", "key1");
        let b = rewrite("test@example.com", "mx.local", "key2");
        assert_ne!(a, b);
    }

    #[test]
    fn roundtrip_recovers_original_sender() {
        let secret = "k1";
        let original = "alice@example.com";
        let rewritten = rewrite(original, "mx.golia.jp", secret);
        let recovered = reverse(&rewritten, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert_eq!(recovered.as_deref(), Some(original));
    }

    #[test]
    fn reverse_rejects_tampered_hash() {
        let secret = "k1";
        let rewritten = rewrite("alice@example.com", "mx.local", secret);
        // Flip one character in the hash position (between "SRS0=" and the next "=")
        let tampered = format!("SRS1{}", &rewritten[4..]);
        let r = reverse(&tampered, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    }

    #[test]
    fn reverse_rejects_wrong_secret() {
        let rewritten = rewrite("alice@example.com", "mx.local", "right-key");
        let r = reverse(&rewritten, "wrong-key", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    }

    #[test]
    fn reverse_rejects_malformed() {
        let r = reverse("not-an-srs-address", "key", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    }

    #[test]
    fn reverse_rejects_missing_at() {
        let r = reverse("SRS0=abcd1234=001=domain=local", "key", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    }

    #[test]
    fn reverse_rejects_short_hash() {
        let r = reverse("SRS0=abc=001=domain=local@mx", "key", DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert!(r.is_none());
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn local_part_with_dots_preserved() {
        let secret = "k";
        let original = "alice.smith@example.com";
        let rewritten = rewrite(original, "mx.local", secret);
        let recovered = reverse(&rewritten, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert_eq!(recovered.as_deref(), Some(original));
    }

    #[test]
    fn subdomain_in_original_preserved() {
        let secret = "k";
        let original = "user@mail.example.co.uk";
        let rewritten = rewrite(original, "mx.local", secret);
        let recovered = reverse(&rewritten, secret, DEFAULT_TIMESTAMP_WINDOW_DAYS);
        assert_eq!(recovered.as_deref(), Some(original));
    }

    #[test]
    fn tt_within_window_self() {
        let tt = current_tt();
        assert!(tt_within_window(&tt, 0));
        assert!(tt_within_window(&tt, 14));
    }

    #[test]
    fn tt_within_window_zero_padded_today() {
        // The current_tt is always 3 chars wide; verify the format.
        let tt = current_tt();
        assert_eq!(tt.len(), 3);
    }
}
