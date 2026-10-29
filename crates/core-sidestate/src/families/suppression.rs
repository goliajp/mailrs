//! Recipient suppression list, served from the network kevy so every
//! sending path consults the same data:
//!
//! ```text
//! mailrs:suppress:{email}   hash — reason, source, added_at
//! ```
//!
//! Why this exists: continuing to deliver to an address that hard-
//! bounced, or to someone who filed a feedback-loop complaint, is the
//! fastest way to lose sending reputation at the large receivers. Both
//! signals are cheap to record and the check costs one HGET per
//! delivery.
//!
//! **Retention differs by source, deliberately.**
//!
//! - A hard bounce is evidence about a mailbox at a point in time.
//!   Mailboxes get recreated, quotas get raised, typo domains get
//!   registered. Suppressing forever would turn a transient truth into
//!   a permanent one, so bounce entries carry a TTL and the address
//!   heals on its own.
//! - A complaint is a statement of intent by a person. It does not
//!   expire, and it is not ours to expire on their behalf, so those
//!   entries are written without a TTL.
//!
//! The TTL also removes the need for a management UI to undo a wrong
//! bounce entry — the common case self-corrects.

use std::time::Duration;

/// How long a hard-bounce suppression lasts before the address is
/// retried. 90 days is the usual industry figure and comfortably
/// outlives a mailbox-full or a temporarily disabled account.
pub const BOUNCE_TTL: Duration = Duration::from_secs(90 * 86_400);

/// What put an address on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A permanent (5.x.x) SMTP failure. Expires after [`BOUNCE_TTL`].
    HardBounce,
    /// An ARF feedback-loop complaint about a message we sent to this
    /// address. Never expires; an operator removes it from the admin
    /// suppression list.
    Complaint,
}

impl Source {
    /// Stored `source` field value.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::HardBounce => "hard_bounce",
            Source::Complaint => "complaint",
        }
    }

    /// TTL to apply, or `None` for an entry that must not expire.
    pub fn ttl(self) -> Option<Duration> {
        match self {
            Source::HardBounce => Some(BOUNCE_TTL),
            Source::Complaint => None,
        }
    }
}

/// Key for one suppressed address.
fn key(email: &str) -> String {
    format!("mailrs:suppress:{}", normalize(email))
}

/// Canonical form for lookups: trimmed, angle brackets stripped, and
/// lowercased. Addresses arrive from SMTP envelopes, ARF reports, and
/// admin input, all shaped slightly differently.
pub fn normalize(email: &str) -> String {
    email
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .trim()
        .to_ascii_lowercase()
}

/// Whether `email` is currently suppressed.
///
/// A kevy error reports `false`. Failing open is deliberate: the cost
/// of one extra message to a bounced address is far lower than
/// silently dropping mail to every recipient because the side-state
/// store is briefly unreachable — deliverability outranks the guard.
pub fn is_suppressed(conn: &mut kevy_client::Connection, email: &str) -> bool {
    conn.hget(key(email).as_bytes(), b"source")
        .ok()
        .flatten()
        .is_some()
}

/// Put `email` on the list, or refresh an existing entry.
///
/// Idempotent by address. Re-recording the same source rewrites the
/// same fields; a complaint arriving after a bounce upgrades the entry
/// to permanent because the complaint's `None` TTL is applied last.
pub fn add(
    conn: &mut kevy_client::Connection,
    email: &str,
    source: Source,
    reason: &str,
    now: i64,
) -> std::io::Result<()> {
    let k = key(email);
    let now_s = now.to_string();
    // Truncate free-text reasons — they come from remote SMTP replies.
    let reason: String = reason.chars().take(200).collect();
    conn.hset(
        k.as_bytes(),
        &[
            (b"source".as_slice(), source.as_str().as_bytes()),
            (b"reason".as_slice(), reason.as_bytes()),
            (b"added_at".as_slice(), now_s.as_bytes()),
        ],
    )?;
    match source.ttl() {
        Some(ttl) => conn.expire(k.as_bytes(), ttl)?,
        // Clear any TTL a previous hard-bounce entry left behind, so a
        // complaint genuinely never expires.
        None => conn.persist(k.as_bytes())?,
    };
    Ok(())
}

/// Take `email` off the list. Returns whether an entry was present.
pub fn remove(conn: &mut kevy_client::Connection, email: &str) -> std::io::Result<bool> {
    let removed = conn.del(&[key(email).as_bytes()])?;
    Ok(removed > 0)
}

/// One entry as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub email: String,
    pub source: String,
    pub reason: String,
    pub added_at: Option<i64>,
}

const PREFIX: &str = "mailrs:suppress:";

/// Every suppressed address with what put it there, sorted by address.
/// The list is a key per address, so this walks them; it is an operator's
/// view, not a delivery-path read.
pub fn list(conn: &mut kevy_client::Connection) -> std::io::Result<Vec<Entry>> {
    let pattern = format!("{PREFIX}*");
    let mut keys = Vec::new();
    let mut cursor = 0;
    loop {
        let (next, batch) = conn.scan(cursor, Some(pattern.as_bytes()), Some(500))?;
        keys.extend(batch);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    let mut out = Vec::new();
    for k in keys {
        let flat = conn.hgetall(&k)?;
        let field = |name: &[u8]| {
            flat.chunks(2)
                .find(|kv| kv.len() == 2 && kv[0] == name)
                .map(|kv| String::from_utf8_lossy(&kv[1]).to_string())
                .unwrap_or_default()
        };
        // an entry that expired between the walk and the read is gone
        if flat.is_empty() {
            continue;
        }
        out.push(Entry {
            email: String::from_utf8_lossy(&k[PREFIX.len()..]).to_string(),
            source: field(b"source"),
            reason: field(b"reason"),
            added_at: field(b"added_at").parse().ok(),
        });
    }
    out.sort_by(|a, b| a.email.cmp(&b.email));
    Ok(out)
}

/// Take every address off the list. Returns how many were removed.
pub fn clear(conn: &mut kevy_client::Connection) -> std::io::Result<usize> {
    let entries = list(conn)?;
    for e in &entries {
        remove(conn, &e.email)?;
    }
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_brackets_and_case() {
        assert_eq!(normalize(" <Alice@Example.COM> "), "alice@example.com");
        assert_eq!(normalize("bob@x.y"), "bob@x.y");
    }

    #[test]
    fn key_uses_the_normalized_address() {
        assert_eq!(key("<A@B.C>"), "mailrs:suppress:a@b.c");
    }

    #[test]
    fn hard_bounce_expires_complaint_does_not() {
        assert_eq!(Source::HardBounce.ttl(), Some(BOUNCE_TTL));
        assert_eq!(Source::Complaint.ttl(), None);
    }

    #[test]
    fn source_strings_are_stable() {
        // Stored in kevy; changing these orphans existing entries.
        assert_eq!(Source::HardBounce.as_str(), "hard_bounce");
        assert_eq!(Source::Complaint.as_str(), "complaint");
    }

    #[test]
    fn bounce_ttl_is_ninety_days() {
        assert_eq!(BOUNCE_TTL.as_secs(), 7_776_000);
    }
}
