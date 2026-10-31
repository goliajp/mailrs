//! Account names and sources as keys and logs keep them.

use std::net::IpAddr;

/// How an account name is kept in a key. Something that is not an address
/// is often a password typed into the wrong field, so only its hash is kept.
pub(super) fn normalize_account(account: &str) -> String {
    let a = account.trim().to_lowercase();
    if !looks_like_address(&a) {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(a.as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        return format!("~{hex}");
    }
    a
}

/// `local@domain.tld` with nothing odd in it.
pub fn looks_like_address(name: &str) -> bool {
    let name = name.trim();
    match name.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
                && name.len() <= 254
                && name.chars().all(|c| c.is_ascii_graphic() || !c.is_ascii())
        }
        None => false,
    }
}

/// An account name as logs may show it: as typed when it is an address,
/// otherwise only its length, since it may well be a password.
pub fn loggable_account(name: &str) -> String {
    match looks_like_address(name) {
        true => name.trim().to_string(),
        false => format!("<{} chars, not an address>", name.chars().count()),
    }
}

/// The network a familiar source stands for: IPv4 /24, IPv6 /64.
pub fn familiar_prefix(source: IpAddr) -> String {
    match crate::normalize_ip(source) {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            format!("{a}.{b}.{c}.0/24")
        }
        v6 => format!("{v6}/64"),
    }
}
