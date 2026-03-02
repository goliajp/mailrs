/// validate an email address (basic RFC 5321 check)
pub fn is_valid(addr: &str) -> bool {
    let Some(at_pos) = addr.find('@') else {
        return false;
    };
    let local = &addr[..at_pos];
    let domain = &addr[at_pos + 1..];
    !local.is_empty() && !domain.is_empty()
}

/// split an email address into (local, domain)
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
