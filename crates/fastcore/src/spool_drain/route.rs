//! Where a recipient's mail goes, worked out without writing anything.
//!
//! The drain delivers along this answer and the RCPT check answers from
//! it, so the receiver refuses exactly the addresses the drain could never
//! deliver. A store that cannot be read is an error, never "nobody": the
//! check must not refuse a real address because kevy was briefly away.

use std::io;

use mailrs_core_sidestate::families::email_groups::{Group, find_by_address};

use super::deliver::{canonical_recipient, has_maildir};
use crate::FastcoreState;

pub(super) enum Route {
    /// the recipient itself: it has a maildir, or is an account without one yet
    Own,
    /// a bounce to an SRS address we wrote, for the original sender
    Srs(String),
    Group(Group),
    /// the alias or domain catch-all target, a mailbox here
    Alias(String),
    Nobody,
}

/// `raw` is the recipient as received, `fwd` its canonical form. The SRS
/// test reads `raw`: both halves of an SRS local part are case-sensitive.
pub(super) fn route(
    state: &FastcoreState,
    maildir_root: &str,
    raw: &str,
    fwd: &str,
) -> io::Result<Route> {
    if is_mailbox(state, maildir_root, fwd)? {
        return Ok(Route::Own);
    }
    if let Some(original) = srs_original(raw) {
        return Ok(Route::Srs(original));
    }
    // before the alias table: a domain catch-all answers for every address
    // without a mailbox and would take the group's mail
    if let Some(group) = group(state, fwd)? {
        return Ok(Route::Group(group));
    }
    if let Some(target) = aliased(state, maildir_root, fwd)? {
        return Ok(Route::Alias(target));
    }
    Ok(match role_fallback(state, maildir_root, fwd)? {
        Some(target) => Route::Alias(target),
        None => Route::Nobody,
    })
}

/// Every domain must take mail for postmaster (RFC 5321 §4.5.1), and abuse
/// is where complaints and feedback loops write (RFC 2142). One that has no
/// route of its own reaches the same address on the first local domain.
fn role_fallback(
    state: &FastcoreState,
    maildir_root: &str,
    fwd: &str,
) -> io::Result<Option<String>> {
    let Some((local, domain)) = fwd.rsplit_once('@') else {
        return Ok(None);
    };
    if !matches!(local, "postmaster" | "abuse") {
        return Ok(None);
    }
    let domains = super::relay::local_domains(state);
    let Some(primary) = domains.first().filter(|p| p.as_str() != domain) else {
        return Ok(None);
    };
    if !domains.iter().any(|d| d == domain) {
        return Ok(None);
    }
    local_mailbox(state, maildir_root, &format!("{local}@{primary}"))
}

/// The mailbox `fwd` reaches without expanding groups: itself, or its
/// alias target.
pub(super) fn local_mailbox(
    state: &FastcoreState,
    maildir_root: &str,
    fwd: &str,
) -> io::Result<Option<String>> {
    if is_mailbox(state, maildir_root, fwd)? {
        return Ok(Some(fwd.to_string()));
    }
    aliased(state, maildir_root, fwd)
}

fn is_mailbox(state: &FastcoreState, maildir_root: &str, addr: &str) -> io::Result<bool> {
    Ok(has_maildir(maildir_root, addr) || state.mailbox.get_account_blob(addr)?.is_some())
}

fn aliased(state: &FastcoreState, maildir_root: &str, fwd: &str) -> io::Result<Option<String>> {
    let Some(target) = state.alias_store.resolve(fwd)? else {
        return Ok(None);
    };
    let target = canonical_recipient(&target);
    Ok(is_mailbox(state, maildir_root, &target)?.then_some(target))
}

/// No network kevy configured means no groups; one that is configured and
/// cannot be reached is an error.
fn group(state: &FastcoreState, fwd: &str) -> io::Result<Option<Group>> {
    let Some(url) = state.net_url.as_deref() else {
        return Ok(None);
    };
    let mut conn = kevy_client::Connection::connect(url).map_err(io::Error::other)?;
    find_by_address(&mut conn, fwd)
}

fn srs_original(raw: &str) -> Option<String> {
    if !raw.to_ascii_uppercase().starts_with("SRS0=") {
        return None;
    }
    let secret = std::env::var("MAILRS_SRS_SECRET").ok()?;
    mailrs_srs::reverse(raw, &secret, mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS)
}
