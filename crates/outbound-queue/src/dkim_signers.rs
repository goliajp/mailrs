//! The domains a loaded [`DkimSignConfig`] can sign for. Keys are parsed
//! lazily on the first message, so a key file that was read but holds no
//! valid key looks configured until mail from its domain fails to sign;
//! this parses every key up front and leaves such a domain out.
//!
//! Tested in `dkim_sign`'s tests, next to the key they sign with.

use std::collections::HashMap;

use crate::dkim_sign::DkimSignConfig;

/// Each domain `cfg` signs as whose key parses → its selector, the default
/// domain included when the default key parses. A per-domain key that does
/// not parse removes its domain even when it is also the default, since
/// that key is the one mail from the domain would be signed with.
pub fn usable_selectors(cfg: &DkimSignConfig) -> HashMap<String, String> {
    let mut out = HashMap::new();
    match cfg.rsa_key() {
        Ok(_) => {
            out.insert(cfg.domain.to_ascii_lowercase(), cfg.selector.clone());
        }
        Err(e) => tracing::error!(domain = %cfg.domain, error = %e, "DKIM: default key unusable"),
    }
    for (domain, key) in &cfg.extra_keys {
        match key.rsa_key() {
            Ok(_) => {
                out.insert(domain.clone(), key.selector.clone());
            }
            Err(e) => {
                tracing::error!(%domain, error = %e, "DKIM: MAILRS_DKIM_KEYS key unusable");
                out.remove(domain);
            }
        }
    }
    out
}
