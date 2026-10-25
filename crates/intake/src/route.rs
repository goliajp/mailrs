//! Where a recipient's mail goes, worked out without writing anything.
//!
//! The drain delivers along this answer and the RCPT check answers from
//! it, so the receiver refuses exactly the addresses the drain could never
//! deliver. A store that cannot be read is an error, never "nobody": the
//! check must not refuse a real address because a store was briefly away.

use std::io;
use std::sync::Arc;

use mailrs_core_sidestate::families::email_groups::Group;

use crate::maildir::{canonical_recipient, has_maildir};

/// What a core knows about its recipients. Each core answers from its own
/// stores; the order they are asked in is [`route`]'s alone.
pub trait Directory {
    /// The maildir root both cores deliver into.
    fn maildir_root(&self) -> &str;
    /// Whether `addr` (canonical) is an account here.
    fn is_account(&self, addr: &str) -> io::Result<bool>;
    /// The target an alias or the domain catch-all sends `addr` to, as
    /// stored. [`route`] checks that the target is a mailbox.
    fn alias(&self, addr: &str) -> io::Result<Option<String>>;
    /// The email group whose address is `addr`. No group store configured
    /// is `Ok(None)`; one that cannot be read is an error.
    fn group(&self, addr: &str) -> io::Result<Option<Group>>;
    /// The domains we host, in their configured order.
    fn local_domains(&self) -> Arc<Vec<String>>;
}

pub enum Route {
    /// the recipient itself: it has a maildir, or is an account without one yet
    Own,
    /// a bounce to an SRS address we wrote, for the original sender
    Srs(String),
    Group(Group),
    /// the mailbox the address's base, an alias or the domain catch-all
    /// leads to
    Alias(String),
    Nobody,
}

/// `raw` is the recipient as received, `fwd` its canonical form. The SRS
/// test reads `raw`: both halves of an SRS local part are case-sensitive.
pub fn route<D: Directory + ?Sized>(dir: &D, raw: &str, fwd: &str) -> io::Result<Route> {
    if is_mailbox(dir, fwd)? {
        return Ok(Route::Own);
    }
    if let Some(original) = srs_original(raw) {
        return Ok(Route::Srs(original));
    }
    // `local+tag@domain` goes where `local@domain` goes, decided before the
    // catch-all can claim it; only a base that reaches nobody falls back to
    // the tagged address as written
    if let Some(base) = base_address(fwd) {
        match route(dir, &base, &base)? {
            Route::Nobody => {}
            Route::Own => return Ok(Route::Alias(base)),
            found => return Ok(found),
        }
    }
    // before the alias table: a domain catch-all answers for every address
    // without a mailbox and would take the group's mail
    if let Some(group) = dir.group(fwd)? {
        return Ok(Route::Group(group));
    }
    if let Some(target) = aliased(dir, fwd)? {
        return Ok(Route::Alias(target));
    }
    Ok(match role_fallback(dir, fwd)? {
        Some(target) => Route::Alias(target),
        None => Route::Nobody,
    })
}

/// `local@domain` for `local+tag@domain`; `None` when there is no tag to
/// strip.
pub fn base_address(addr: &str) -> Option<String> {
    let (local, domain) = addr.rsplit_once('@')?;
    let (base, _tag) = local.split_once('+')?;
    (!base.is_empty()).then(|| format!("{base}@{domain}"))
}

/// Every domain must take mail for postmaster (RFC 5321 §4.5.1), and abuse
/// is where complaints and feedback loops write (RFC 2142). One that has no
/// route of its own reaches the same address on the first local domain.
fn role_fallback<D: Directory + ?Sized>(dir: &D, fwd: &str) -> io::Result<Option<String>> {
    let Some((local, domain)) = fwd.rsplit_once('@') else {
        return Ok(None);
    };
    if !matches!(local, "postmaster" | "abuse") {
        return Ok(None);
    }
    let domains = dir.local_domains();
    let Some(primary) = domains.first().filter(|p| p.as_str() != domain) else {
        return Ok(None);
    };
    if !domains.iter().any(|d| d == domain) {
        return Ok(None);
    }
    local_mailbox(dir, &format!("{local}@{primary}"))
}

/// The mailbox `fwd` reaches without expanding groups: itself, or its
/// alias target.
pub fn local_mailbox<D: Directory + ?Sized>(dir: &D, fwd: &str) -> io::Result<Option<String>> {
    if is_mailbox(dir, fwd)? {
        return Ok(Some(fwd.to_string()));
    }
    aliased(dir, fwd)
}

/// A maildir on disk, or an account that has none yet.
pub fn is_mailbox<D: Directory + ?Sized>(dir: &D, addr: &str) -> io::Result<bool> {
    Ok(has_maildir(dir.maildir_root(), addr) || dir.is_account(addr)?)
}

fn aliased<D: Directory + ?Sized>(dir: &D, fwd: &str) -> io::Result<Option<String>> {
    let Some(target) = dir.alias(fwd)? else {
        return Ok(None);
    };
    let target = canonical_recipient(target.trim());
    Ok(is_mailbox(dir, &target)?.then_some(target))
}

fn srs_original(raw: &str) -> Option<String> {
    if !raw.to_ascii_uppercase().starts_with("SRS0=") {
        return None;
    }
    let secret = std::env::var("MAILRS_SRS_SECRET").ok()?;
    mailrs_srs::reverse(raw, &secret, mailrs_srs::DEFAULT_TIMESTAMP_WINDOW_DAYS)
}
