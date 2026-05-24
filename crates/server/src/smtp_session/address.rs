/// check if a domain is in the local domains list
/// if list is empty, all domains are considered local (backwards compatible)
pub(super) fn is_local_domain(domain: &str, local_domains: &[String]) -> bool {
    if local_domains.is_empty() {
        return true;
    }
    let domain_lower = domain.to_lowercase();
    local_domains.contains(&domain_lower)
}

#[cfg(test)]
#[path = "address_tests.rs"]
mod tests;
