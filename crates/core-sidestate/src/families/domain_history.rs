//! How much mail this deployment has ever had from a domain.
//!
//! One counter, deployment-wide rather than per user:
//!
//! ```text
//! mailrs:domseen:{registrable_domain}   integer
//! ```
//!
//! It exists for the brand-impersonation check
//! ([`mailrs_fraud::brand`]), which needs to tell a phish claiming to
//! be Amazon from a newsletter that merely names it. Measured over
//! 35,962 production messages, every one of the twenty phishing
//! senders came from a domain seen once or twice and every one of the
//! twelve legitimate ones from a domain seen three times or more.
//!
//! **Not per user.** "Have we ever corresponded with this domain" is a
//! property of the whole install, and a user with little history is
//! exactly the one the signal has to work for. It is also the reason
//! the counter is cheap: one key per domain, not one per (user,
//! domain) pair.
//!
//! **Registrable domain, not the full host.** The phish rotates the
//! host as well — `mail02.marriottanji.com`, `mail32.jihuanengyuan.com`
//! — so counting hosts would make every sender unfamiliar, including
//! the legitimate ones that use per-campaign subdomains.

/// Count one more message from `host`.
///
/// Called once per delivered message, at ingest. Best-effort: a
/// counter that failed to increment makes a domain look less familiar
/// than it is, which holds mail rather than delivering it — the safe
/// direction to be wrong in, and the reason this returns `()`.
pub fn record_seen(conn: &mut kevy_client::Connection, host: &str) {
    let d = mailrs_fraud::brand::registrable(host);
    if d.is_empty() {
        return;
    }
    let _ = conn.incr(mailrs_fraud::brand::seen_key(&d).as_bytes());
}

/// How many messages this deployment has had from `host`'s domain.
///
/// Zero when the key is missing, which is the honest answer for a
/// domain nothing has ever arrived from — and the answer that makes a
/// brand claim from it suspicious.
pub fn seen(conn: &mut kevy_client::Connection, host: &str) -> u64 {
    let d = mailrs_fraud::brand::registrable(host);
    if d.is_empty() {
        return 0;
    }
    conn.get(mailrs_fraud::brand::seen_key(&d).as_bytes())
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8_lossy(&v).trim().parse().ok())
        .unwrap_or(0)
}
