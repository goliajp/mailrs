//! Which `d=` and selector the sender signs a domain's mail with.
//!
//! The answer comes from what the sender published at its last start
//! (`families::dkim_signers`): it lists only the keys that loaded. Before
//! a sender has published, the same three variables the sender reads are
//! used instead; webapi cannot open the key files, so an entry whose file
//! the sender cannot load still looks configured there.

use std::collections::HashMap;

use mailrs_core_sidestate::families::dkim_signers::Published;
use mailrs_dkim::key_list::{entry_for_domain, parse_key_list};

/// How mail from one domain is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signing {
    /// signed with this `d=` and selector
    By(Signer),
    /// the sender has no DKIM: mail goes out unsigned
    Unsigned,
    /// the default key the sender would fall back to did not load, so
    /// mail from the domain cannot be signed and is refused
    DefaultKeyBroken(String),
    /// the sender has not published and webapi's environment does not say
    Unknown,
}

/// [`Signing`] for one domain, with what is known about how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DkimFinding {
    pub signing: Signing,
    /// `MAILRS_DKIM_KEYS` lists a key for the domain that the sender did
    /// not load
    pub key_not_loaded: bool,
    /// set when the answer is read from webapi's environment
    pub note: Option<String>,
}

pub const NOT_PUBLISHED: &str = "The sender has not reported its DKIM keys since it last started, so this is read from webapi's environment and cannot tell whether a key file loaded";

/// How mail from `domain` is signed: from what the sender published when
/// there is that, from `env` otherwise.
pub fn finding(
    published: Option<&Published>,
    env: Option<&DkimSigning>,
    domain: &str,
) -> DkimFinding {
    let domain = domain.to_ascii_lowercase();
    let Some(p) = published else {
        let signing = match env {
            Some(e) => Signing::By(e.signer_for(&domain)),
            None => Signing::Unknown,
        };
        return DkimFinding {
            signing,
            key_not_loaded: false,
            note: Some(NOT_PUBLISHED.into()),
        };
    };
    let selectors = &p.signers.selectors;
    let signing = match (
        entry_for_domain(selectors, &domain),
        &p.signers.default_domain,
    ) {
        (Some((d, selector)), _) => Signing::By(Signer {
            domain: d.to_string(),
            selector: selector.clone(),
        }),
        (None, None) => Signing::Unsigned,
        (None, Some(default)) => match selectors.get(default) {
            Some(selector) => Signing::By(Signer {
                domain: default.clone(),
                selector: selector.clone(),
            }),
            None => Signing::DefaultKeyBroken(default.clone()),
        },
    };
    let listed = env.is_some_and(|e| entry_for_domain(&e.per_domain, &domain).is_some());
    DkimFinding {
        key_not_loaded: listed && entry_for_domain(selectors, &domain).is_none(),
        signing,
        note: None,
    }
}

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

    fn published(default: Option<&str>, pairs: &[(&str, &str)]) -> Published {
        Published {
            signers: mailrs_core_sidestate::families::dkim_signers::DkimSigners {
                default_domain: default.map(str::to_string),
                selectors: pairs
                    .iter()
                    .map(|(d, s)| (d.to_string(), s.to_string()))
                    .collect(),
            },
            published_at: 1,
        }
    }

    #[test]
    fn a_published_domain_is_signed_with_its_own_key() {
        let p = published(
            Some("golia.jp"),
            &[("golia.jp", "mail"), ("golia.ai", "s2")],
        );
        let f = finding(Some(&p), None, "News.Golia.AI");
        assert_eq!(f.signing, Signing::By(signer("golia.ai", "s2")));
        assert!(!f.key_not_loaded);
        assert_eq!(f.note, None);
        let f = finding(Some(&p), None, "golia.jp");
        assert_eq!(f.signing, Signing::By(signer("golia.jp", "mail")));
    }

    #[test]
    fn an_unpublished_domain_is_signed_as_the_default_and_a_listed_one_did_not_load() {
        let p = published(Some("golia.jp"), &[("golia.jp", "mail")]);
        let env = DkimSigning::parse("golia.jp", "mail", "golia.ai:s2:/k");
        let f = finding(Some(&p), Some(&env), "golia.ai");
        assert_eq!(f.signing, Signing::By(signer("golia.jp", "mail")));
        assert!(f.key_not_loaded, "MAILRS_DKIM_KEYS lists golia.ai");
        let f = finding(Some(&p), Some(&env), "new-domain.com");
        assert_eq!(f.signing, Signing::By(signer("golia.jp", "mail")));
        assert!(!f.key_not_loaded);
    }

    #[test]
    fn no_dkim_is_unsigned_and_a_default_key_that_did_not_load_is_broken() {
        let f = finding(Some(&published(None, &[])), None, "golia.ai");
        assert_eq!(f.signing, Signing::Unsigned);
        let p = published(Some("golia.jp"), &[("golia.ai", "s2")]);
        let f = finding(Some(&p), None, "dadaya.jp");
        assert_eq!(f.signing, Signing::DefaultKeyBroken("golia.jp".into()));
        let f = finding(Some(&p), None, "golia.jp");
        assert_eq!(f.signing, Signing::DefaultKeyBroken("golia.jp".into()));
    }

    #[test]
    fn nothing_published_falls_back_to_the_environment_with_a_note() {
        let env = DkimSigning::parse("golia.jp", "mail", "golia.ai:s2:/k");
        let f = finding(None, Some(&env), "golia.ai");
        assert_eq!(f.signing, Signing::By(signer("golia.ai", "s2")));
        assert_eq!(f.note.as_deref(), Some(NOT_PUBLISHED));
        let f = finding(None, None, "golia.ai");
        assert_eq!(f.signing, Signing::Unknown);
        assert_eq!(f.note.as_deref(), Some(NOT_PUBLISHED));
    }
}
