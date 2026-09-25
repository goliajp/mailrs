//! The display names on this deployment's own account rows.
//!
//! ```text
//! mailrs:accountnames   set of display names
//! ```
//!
//! Published by fastcore, which holds the account store, and read by
//! the receiver, which does not. One writer and one source — not a
//! second copy in a variable somebody has to keep in step, which is
//! the arrangement that already failed once: `MAILRS_ORG_NAMES` was
//! set on the receiver and not on fastcore, and half the
//! impersonation check was silently off for a day
//! (`.claude/rules/both-halves-of-the-wire.md`).
//!
//! Read by [`mailrs_fraud::impersonation::impersonates_one_of_us`],
//! which convicts a display name that **is** one of these from a
//! domain that is not ours. Empty turns that check off, so a
//! deployment that cannot reach the store delivers rather than holds.

/// The key both sides use. Spelled once.
pub const KEY: &str = "mailrs:accountnames";

/// Replace the published set with `names`.
///
/// Whole-set replacement rather than additions, so a renamed or
/// deleted account stops being published. Cheap: thirteen names here.
///
/// Best-effort. A set that failed to publish makes the check see
/// nothing, which delivers rather than holds — the safe direction,
/// and why this returns `()`.
pub fn publish(conn: &mut kevy_client::Connection, names: &[String]) {
    let members: Vec<&[u8]> = names
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .map(str::as_bytes)
        .collect();
    if members.is_empty() {
        return;
    }
    let _ = conn.del(&[KEY.as_bytes()]);
    let _ = conn.sadd(KEY.as_bytes(), &members);
}

/// What was published, or nothing.
#[must_use]
pub fn read(conn: &mut kevy_client::Connection) -> Vec<String> {
    conn.smembers(KEY.as_bytes())
        .unwrap_or_default()
        .into_iter()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .filter(|s| !s.trim().is_empty())
        .collect()
}
