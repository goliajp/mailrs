//! External identities linked to mailrs accounts.
//!
//! ```text
//!   oidc:link:{issuer}:{subject}   -> account address
//!   oidc:links:{address}           set of "{issuer}|{subject}"
//!   oidc:pending:{handle}          identity awaiting a link, TTL, single-use
//! ```
//!
//! Third-party login is an authentication method, not a source of accounts:
//! an account exists in mailrs or it does not, and signing in with Google is
//! another way of proving you own one. Nothing here creates an account, and
//! nothing here looks an account up by email.
//!
//! The link is keyed on `(issuer, subject)` and never on the email address.
//! An email is a provider's opinion about a string — GitHub's is typed by the
//! account holder and unverified, Apple's is a per-app relay — and the
//! subject is the only part that is both stable and the provider's own. See
//! `.claude/rfcs/20260801-external-login.md`.

use kevy_client::Connection;

const LINK_PREFIX: &str = "oidc:link:";
const LINKS_PREFIX: &str = "oidc:links:";
const PENDING_PREFIX: &str = "oidc:pending:";

/// How long an identity waits to be claimed by a password login.
///
/// Long enough to type a password and a TOTP code, short enough that a
/// handle captured from a shared machine is stale before it is useful.
pub const PENDING_TTL: std::time::Duration = std::time::Duration::from_secs(600);

fn link_key(issuer: &str, subject: &str) -> String {
    format!("{LINK_PREFIX}{issuer}:{subject}")
}

fn links_key(address: &str) -> String {
    format!("{LINKS_PREFIX}{address}")
}

/// The stored form of one link, for listing and for revocation.
pub fn member(issuer: &str, subject: &str) -> String {
    format!("{issuer}|{subject}")
}

/// Which account this identity signs in as, if any.
///
/// `None` is the answer for every identity nobody has linked, and it is not
/// an error — it is the branch that sends the user to a password login.
pub fn account_for(
    conn: &mut Connection,
    issuer: &str,
    subject: &str,
) -> std::io::Result<Option<String>> {
    let raw = conn
        .get(link_key(issuer, subject).as_bytes())
        .map_err(std::io::Error::other)?;
    Ok(raw.and_then(|b| String::from_utf8(b).ok()))
}

/// Link an identity to an account.
///
/// Refuses to move a link that already points somewhere else. Re-pointing is
/// not an edit — it is one person taking over another's sign-in — so it has
/// to go through an explicit unlink by whoever holds it now.
pub fn link(
    conn: &mut Connection,
    issuer: &str,
    subject: &str,
    address: &str,
) -> std::io::Result<LinkOutcome> {
    if let Some(existing) = account_for(conn, issuer, subject)? {
        if existing == address {
            return Ok(LinkOutcome::AlreadyLinked);
        }
        return Ok(LinkOutcome::TakenByAnotherAccount);
    }
    conn.set(link_key(issuer, subject).as_bytes(), address.as_bytes())
        .map_err(std::io::Error::other)?;
    let m = member(issuer, subject);
    conn.sadd(links_key(address).as_bytes(), &[m.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(LinkOutcome::Linked)
}

/// What `link` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// Written.
    Linked,
    /// The same account already had it — nothing to do.
    AlreadyLinked,
    /// A different account holds it. Never silently moved.
    TakenByAnotherAccount,
}

/// Remove a link. Returns whether one was there.
pub fn unlink(
    conn: &mut Connection,
    issuer: &str,
    subject: &str,
    address: &str,
) -> std::io::Result<bool> {
    let existing = account_for(conn, issuer, subject)?;
    // Only the holder may remove it, so a caller cannot unlink somebody
    // else's identity by naming it.
    if existing.as_deref() != Some(address) {
        return Ok(false);
    }
    conn.del(&[link_key(issuer, subject).as_bytes()])
        .map_err(std::io::Error::other)?;
    let m = member(issuer, subject);
    conn.srem(links_key(address).as_bytes(), &[m.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(true)
}

/// Every identity linked to an account, as `(issuer, subject)`.
pub fn links_for(conn: &mut Connection, address: &str) -> std::io::Result<Vec<(String, String)>> {
    let members = conn
        .smembers(links_key(address).as_bytes())
        .map_err(std::io::Error::other)?;
    Ok(members
        .into_iter()
        .filter_map(|m| String::from_utf8(m).ok())
        .filter_map(|m| split_member(&m))
        .collect())
}

/// Split a stored member back into its parts.
///
/// On the **first** separator: an issuer is a URL and contains none, while a
/// subject may contain anything a provider chooses. Splitting on the last one
/// would corrupt any subject that has a `|` in it.
pub fn split_member(m: &str) -> Option<(String, String)> {
    let (issuer, subject) = m.split_once('|')?;
    if issuer.is_empty() || subject.is_empty() {
        return None;
    }
    Some((issuer.to_string(), subject.to_string()))
}

/// Drop every link on an account.
///
/// Called when a password is reset: whoever reset it may be recovering from
/// a compromise, and a link left in place is a way back in that changing the
/// password did not close.
pub fn unlink_all(conn: &mut Connection, address: &str) -> std::io::Result<usize> {
    let all = links_for(conn, address)?;
    let mut removed = 0usize;
    for (issuer, subject) in &all {
        conn.del(&[link_key(issuer, subject).as_bytes()])
            .map_err(std::io::Error::other)?;
        removed += 1;
    }
    conn.del(&[links_key(address).as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(removed)
}

/// Park an identity that has authenticated but is not linked to anything.
///
/// Returned handle goes to the browser in a cookie — never in a URL, where it
/// would survive in history and in the referrer sent to the next site. The
/// entry expires on its own and is deleted when claimed.
pub fn park_pending(
    conn: &mut Connection,
    handle: &str,
    identity_json: &str,
) -> std::io::Result<()> {
    conn.set_with_ttl(
        format!("{PENDING_PREFIX}{handle}").as_bytes(),
        identity_json.as_bytes(),
        PENDING_TTL,
    )
    .map_err(std::io::Error::other)
}

/// Take a parked identity, if the handle names one.
///
/// Deletes it in the same call. Single-use is what stops a captured handle
/// being replayed later to link the same identity to a second account —
/// leaving it readable would make the ten-minute window a ten-minute window
/// for anyone holding the cookie.
pub fn claim_pending(conn: &mut Connection, handle: &str) -> std::io::Result<Option<String>> {
    let key = format!("{PENDING_PREFIX}{handle}");
    let raw = conn.get(key.as_bytes()).map_err(std::io::Error::other)?;
    if raw.is_some() {
        conn.del(&[key.as_bytes()]).map_err(std::io::Error::other)?;
    }
    Ok(raw.and_then(|b| String::from_utf8(b).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An issuer is a URL and has no `|`; a subject may have anything.
    #[test]
    fn a_member_splits_on_the_first_separator() {
        assert_eq!(
            split_member("https://portal.golia.jp|user-42"),
            Some(("https://portal.golia.jp".into(), "user-42".into()))
        );
        // A subject containing the separator must survive intact.
        assert_eq!(
            split_member("https://x|a|b"),
            Some(("https://x".into(), "a|b".into()))
        );
        assert_eq!(split_member("nosep"), None);
        assert_eq!(split_member("|empty-issuer"), None);
        assert_eq!(split_member("empty-subject|"), None);
    }

    #[test]
    fn the_member_form_round_trips() {
        let (i, s) = ("https://accounts.google.com", "1029384756");
        assert_eq!(split_member(&member(i, s)), Some((i.into(), s.into())));
    }

    /// The key is the identity, not the address — an email is a provider's
    /// opinion about a string and changes; the subject does not.
    #[test]
    fn the_link_key_names_the_identity() {
        let k = link_key("https://accounts.google.com", "1029384756");
        assert!(k.starts_with("oidc:link:"));
        assert!(k.contains("accounts.google.com"));
        assert!(k.ends_with("1029384756"));
    }

    /// Long enough to type a password and a TOTP code, short enough that a
    /// handle from a shared machine is stale before it is useful.
    #[test]
    fn the_pending_window_is_minutes_not_hours() {
        assert!(PENDING_TTL.as_secs() >= 300);
        assert!(PENDING_TTL.as_secs() <= 900);
    }
}
