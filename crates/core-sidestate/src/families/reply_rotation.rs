//! Which sending domains funnel into one reply address.
//!
//! ```text
//! mailrs:replyrot:{reply_address}   set of registrable sending domains
//! ```
//!
//! Deployment-wide, like [`super::domain_history`], and for the same
//! reason: how many disposable domains a sender burns is a property
//! of the sender, and a user with little history is exactly the one
//! the signal has to work for.
//!
//! A set rather than a counter, because the question is *how many
//! different domains*, not how much mail. The fortune-telling
//! campaign sent 179 messages from 18 hosts; what convicts it is the
//! **six registered domains**, and 179 would have said nothing.
//!
//! See [`mailrs_fraud::reply_rotation`] for the measurement and the
//! threshold.

/// Record that `from_host` sent a message whose replies go to
/// `reply_addr`.
///
/// A no-op when the reply address is inside the sender's own estate,
/// which is what ordinary mail does — that filter is what keeps this
/// to a few hundred keys instead of one per sender.
///
/// Best-effort. A set that failed to grow makes a rotating sender
/// look settled, which delivers mail rather than hiding it — the safe
/// direction, and why this returns `()`.
pub fn record(conn: &mut kevy_client::Connection, from_host: &str, reply_addr: &str) {
    if !mailrs_fraud::reply_rotation::is_off_domain(from_host, reply_addr) {
        return;
    }
    let domain = mailrs_fraud::brand::registrable(from_host);
    if domain.is_empty() {
        return;
    }
    let key = mailrs_fraud::reply_rotation::rotation_key(reply_addr);
    let _ = conn.sadd(key.as_bytes(), &[domain.as_bytes()]);
}

/// How many distinct registrable domains send to `reply_addr`.
///
/// Zero when the reply address is the sender's own, or when nothing
/// has been recorded — both of which read as *not rotating*, so a
/// deployment with no history delivers.
#[must_use]
pub fn domains(conn: &mut kevy_client::Connection, from_host: &str, reply_addr: &str) -> u32 {
    if !mailrs_fraud::reply_rotation::is_off_domain(from_host, reply_addr) {
        return 0;
    }
    let key = mailrs_fraud::reply_rotation::rotation_key(reply_addr);
    conn.scard(key.as_bytes())
        .unwrap_or(0)
        .try_into()
        .unwrap_or(u32::MAX)
}
