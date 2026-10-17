//! Which `d=` and selector the sender signs a domain's mail with, from
//! the same three variables the sender reads. Key files are not read:
//! webapi has no access to them, so an entry whose file the sender
//! cannot open still looks configured here.

use std::collections::HashMap;

use mailrs_dkim::key_list::{entry_for_domain, parse_key_list};

/// The signing setup, as `MAILRS_DKIM_DOMAIN`, `MAILRS_DKIM_SELECTOR`
/// and `MAILRS_DKIM_KEYS` describe it.
pub struct DkimSigning {
    pub default_domain: String,
    pub default_selector: String,
    /// domain → selector, from `MAILRS_DKIM_KEYS`
    pub per_domain: HashMap<String, String>,
}

/// The `d=` and selector mail from a domain is signed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    pub domain: String,
    pub selector: String,
}

impl Signer {
    /// Whether a signature with this `d=` aligns with mail from `domain`
    /// under relaxed alignment: the same domain or a parent of it.
    pub fn aligns_with(&self, domain: &str) -> bool {
        let domain = domain.to_ascii_lowercase();
        domain == self.domain || domain.ends_with(&format!(".{}", self.domain))
    }
}

impl DkimSigning {
    /// `None` when the default domain or selector is unset: the sender
    /// then signs nothing.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Some(Self::parse(
            &var("MAILRS_DKIM_DOMAIN")?,
            &var("MAILRS_DKIM_SELECTOR")?,
            &var("MAILRS_DKIM_KEYS").unwrap_or_default(),
        ))
    }

    pub fn parse(default_domain: &str, default_selector: &str, keys: &str) -> Self {
        let per_domain = parse_key_list(keys)
            .into_iter()
            .flatten()
            .map(|e| (e.domain, e.selector.to_string()))
            .collect();
        Self {
            default_domain: default_domain.to_ascii_lowercase(),
            default_selector: default_selector.to_string(),
            per_domain,
        }
    }

    pub fn signer_for(&self, domain: &str) -> Signer {
        let domain = domain.to_ascii_lowercase();
        match entry_for_domain(&self.per_domain, &domain) {
            Some((d, selector)) => Signer {
                domain: d.to_string(),
                selector: selector.clone(),
            },
            None => Signer {
                domain: self.default_domain.clone(),
                selector: self.default_selector.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer(domain: &str, selector: &str) -> Signer {
        Signer {
            domain: domain.into(),
            selector: selector.into(),
        }
    }

    #[test]
    fn listed_domains_use_their_own_selector_and_the_rest_the_default() {
        let s = DkimSigning::parse("golia.jp", "mail", "golia.ai:s2:/k,broken,dadaya.jp:s3:/k");
        assert_eq!(s.signer_for("GOLIA.AI"), signer("golia.ai", "s2"));
        assert_eq!(s.signer_for("dadaya.jp"), signer("dadaya.jp", "s3"));
        assert_eq!(s.signer_for("news.golia.ai"), signer("golia.ai", "s2"));
        assert_eq!(s.signer_for("golia.jp"), signer("golia.jp", "mail"));
        assert_eq!(s.signer_for("new-domain.com"), signer("golia.jp", "mail"));
    }

    #[test]
    fn alignment_is_the_same_domain_or_a_parent() {
        let s = signer("golia.ai", "mail");
        assert!(s.aligns_with("golia.ai"));
        assert!(s.aligns_with("News.Golia.AI"));
        assert!(!s.aligns_with("xgolia.ai"));
        assert!(!s.aligns_with("golia.jp"));
    }
}
