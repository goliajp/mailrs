//! Domains a threshold never lists: shared platforms, link shorteners and
//! this server's own domains. Data the rules read, reviewed on its own.

use std::collections::HashSet;

/// The never-list.
#[derive(Debug, Clone, Default)]
pub struct NeverList {
    domains: HashSet<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    domains: Vec<String>,
}

impl NeverList {
    /// The shipped list, `never_list.toml`, plus `own` (this server's
    /// domains).
    pub fn builtin(own: &[String]) -> NeverList {
        let raw: Raw = toml::from_str(include_str!("../never_list.toml"))
            .expect("the shipped never-list parses");
        let domains = raw
            .domains
            .iter()
            .chain(own)
            .map(|d| crate::registrable(d))
            .collect();
        NeverList { domains }
    }

    /// Whether a host's registrable domain is on the list.
    pub fn contains(&self, host: &str) -> bool {
        self.domains.contains(&crate::registrable(host))
    }

    /// The sha256 of the shipped list, for health.
    pub fn sha256() -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(include_str!("../never_list.toml").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}
