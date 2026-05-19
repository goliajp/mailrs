//! Minimal email address helpers (validation and local/domain split).
//!
//! These are deliberately permissive — they only check the `local@domain`
//! shape, not RFC 5321 syntax in full. For strict validation use a dedicated
//! parser.

/// Return `true` if `addr` contains a non-empty local part, an `@`, and a
/// non-empty domain part.
pub fn is_valid(addr: &str) -> bool {
    let Some(at_pos) = addr.find('@') else {
        return false;
    };
    let local = &addr[..at_pos];
    let domain = &addr[at_pos + 1..];
    !local.is_empty() && !domain.is_empty()
}

/// Split an email address into `(local, domain)` at the first `@`, or
/// return `None` if either side is empty.
pub fn split_address(addr: &str) -> Option<(&str, &str)> {
    let at_pos = addr.find('@')?;
    let local = &addr[..at_pos];
    let domain = &addr[at_pos + 1..];
    if local.is_empty() || domain.is_empty() {
        return None;
    }
    Some((local, domain))
}

#[cfg(test)]
mod tests;
