//! Whether two hosts belong to one organisation, by the public suffix
//! list, and whether a message's DKIM signatures speak for its `From:`.
//!
//! [`crate::brand::registrable`] knows a handful of Japanese suffixes and
//! takes the last two labels otherwise, which puts `shop-a.com.cn` and
//! `shop-b.com.cn` in one organisation. Comparing two parties needs the
//! real list.

/// The registrable domain of `host` by the public suffix list; the
/// lowercased host itself when the list cannot place it.
#[must_use]
pub fn of(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match psl::domain_str(&host) {
        Some(org) => org.to_string(),
        None => host,
    }
}

/// A `Reply-To:` address on another organisation than the `From:` host.
/// False when either side is missing: no reply address diverts nothing.
#[must_use]
pub fn reply_elsewhere(from_host: &str, reply_addr: &str) -> bool {
    let Some((_, reply_host)) = reply_addr.rsplit_once('@') else {
        return false;
    };
    if reply_host.is_empty() || from_host.is_empty() {
        return false;
    }
    of(from_host) != of(reply_host)
}

/// The message carries at least one DKIM signature and every one is by
/// the `From:` organisation.
///
/// The stored DKIM result is one word for all signatures, so with a
/// foreign signer present a `pass` does not say whose signature passed;
/// only when every signer is the `From:` organisation does a `pass` mean
/// an aligned signature verified.
#[must_use]
pub fn signed_only_by(from_host: &str, signers: &[String]) -> bool {
    let from = of(from_host);
    !from.is_empty() && !signers.is_empty() && signers.iter().all(|d| of(d) == from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomains_share_their_organisation() {
        assert_eq!(of("mail.nvidia.cn"), "nvidia.cn");
        assert_eq!(of("e.stripe.com"), "stripe.com");
        assert_eq!(of("news.example.co.uk"), "example.co.uk");
        assert!(!reply_elsewhere("mail.nvidia.cn", "events@nvidia.cn"));
    }

    #[test]
    fn a_shared_public_suffix_is_not_an_organisation() {
        assert!(reply_elsewhere("shop-a.com.cn", "x@shop-b.com.cn"));
    }

    #[test]
    fn a_missing_side_diverts_nothing() {
        assert!(!reply_elsewhere("a.example", ""));
        assert!(!reply_elsewhere("a.example", "no-at-sign"));
        assert!(!reply_elsewhere("", "x@b.example"));
    }

    #[test]
    fn every_signer_must_be_the_from_organisation() {
        let own = vec!["nvidia.cn".to_string()];
        let mixed = vec!["nvidia.cn".to_string(), "esp.example".to_string()];
        assert!(signed_only_by("mail.nvidia.cn", &own));
        assert!(!signed_only_by("mail.nvidia.cn", &mixed));
        assert!(!signed_only_by("mail.nvidia.cn", &[]));
    }
}
