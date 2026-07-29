//! Build a [`DkimSignConfig`]'s per-domain key map from the
//! environment.
//!
//! Lives here rather than in a binary so both the fastcore sender and
//! the monolith read `MAILRS_DKIM_KEYS` through the same code. A
//! previous split — the monolith parsing it in its config loader while
//! `fastcore/bin/sender.rs` hardcoded an empty map — meant production
//! signed every outbound message with the default `d=`, silently
//! breaking DKIM alignment for every hosted domain except the default
//! one. Alignment failures only surface in forwarding, so nothing
//! caught it.
//!
//! Format: `domain:selector:path[,domain:selector:path...]`
//!
//! A malformed entry, an empty field, or an unreadable key file skips
//! that one entry with a warning. One bad line must not take signing
//! down for every other domain — an unsigned message is far more likely
//! to be rejected than a message signed for fewer domains than intended.

use std::collections::HashMap;

use crate::dkim_sign::DkimDomainKey;

/// Read `MAILRS_DKIM_KEYS` and build the per-domain key map.
///
/// Returns an empty map when the variable is unset or empty — which is
/// exactly the single-domain configuration, where signing falls back to
/// the config's default `domain` / `selector` / `private_key_pem`.
pub fn extra_keys_from_env() -> HashMap<String, DkimDomainKey> {
    match std::env::var("MAILRS_DKIM_KEYS") {
        Ok(raw) => parse_extra_keys(&raw, |path| std::fs::read_to_string(path)),
        Err(_) => HashMap::new(),
    }
}

/// Testable core of [`extra_keys_from_env`].
///
/// `read_key` supplies a key file's contents, so tests can exercise the
/// parsing and the skip-on-error paths without touching a filesystem.
fn parse_extra_keys<F>(raw: &str, mut read_key: F) -> HashMap<String, DkimDomainKey>
where
    F: FnMut(&str) -> std::io::Result<String>,
{
    let mut out = HashMap::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let parts: Vec<&str> = entry.splitn(3, ':').collect();
        if parts.len() != 3 {
            tracing::warn!(
                event = "config_error",
                entry,
                "MAILRS_DKIM_KEYS entry skipped (expected domain:selector:path)"
            );
            continue;
        }
        let domain = parts[0].trim();
        let selector = parts[1].trim();
        let path = parts[2].trim();
        if domain.is_empty() || selector.is_empty() || path.is_empty() {
            tracing::warn!(
                event = "config_error",
                entry,
                "MAILRS_DKIM_KEYS entry skipped (empty field)"
            );
            continue;
        }
        let pem = match read_key(path) {
            Ok(pem) => pem,
            Err(e) => {
                tracing::warn!(
                    event = "config_error",
                    entry,
                    error = %e,
                    "MAILRS_DKIM_KEYS entry skipped (key file unreadable)"
                );
                continue;
            }
        };
        out.insert(
            domain.to_ascii_lowercase(),
            DkimDomainKey {
                selector: selector.to_string(),
                private_key_pem: pem,
                parsed_key: Default::default(),
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always(pem: &'static str) -> impl FnMut(&str) -> std::io::Result<String> {
        move |_| Ok(pem.to_string())
    }

    #[test]
    fn parses_a_single_entry() {
        let keys = parse_extra_keys("golia.ai:mail:/dkim/golia.jp.key", always("PEM"));

        assert_eq!(keys.len(), 1);
        let k = keys.get("golia.ai").expect("entry present");
        assert_eq!(k.selector, "mail");
        assert_eq!(k.private_key_pem, "PEM");
    }

    #[test]
    fn parses_every_hosted_domain() {
        let raw = "golia.ai:mail:/k,dadaya.jp:mail:/k,bitreits.com:mail:/k,\
                   doracawl.com:mail:/k,madcawl.com:mail:/k,marspot.com:mail:/k";

        let keys = parse_extra_keys(raw, always("PEM"));

        assert_eq!(keys.len(), 6);
        for d in [
            "golia.ai",
            "dadaya.jp",
            "bitreits.com",
            "doracawl.com",
            "madcawl.com",
            "marspot.com",
        ] {
            assert!(keys.contains_key(d), "{d} must be present");
        }
    }

    #[test]
    fn tolerates_whitespace_and_trailing_separators() {
        let keys = parse_extra_keys(
            " golia.ai : mail : /k , , dadaya.jp:mail:/k ,",
            always("PEM"),
        );

        assert_eq!(keys.len(), 2);
        assert!(keys.contains_key("golia.ai"));
        assert!(keys.contains_key("dadaya.jp"));
    }

    #[test]
    fn lowercases_the_domain_key() {
        // From-header domains are compared lowercased at sign time.
        let keys = parse_extra_keys("GOLIA.AI:mail:/k", always("PEM"));

        assert!(keys.contains_key("golia.ai"));
        assert!(!keys.contains_key("GOLIA.AI"));
    }

    #[test]
    fn keeps_a_path_containing_colons() {
        // splitn(3) — only the first two colons separate fields.
        let keys = parse_extra_keys("golia.ai:mail:/a:b/c.key", |path| {
            assert_eq!(path, "/a:b/c.key");
            Ok("PEM".into())
        });

        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn skips_a_malformed_entry_but_keeps_the_rest() {
        let keys = parse_extra_keys(
            "golia.ai:mail:/k,broken-entry,dadaya.jp:mail:/k",
            always("PEM"),
        );

        assert_eq!(keys.len(), 2, "one bad entry must not lose the good ones");
        assert!(keys.contains_key("golia.ai"));
        assert!(keys.contains_key("dadaya.jp"));
    }

    #[test]
    fn skips_an_entry_with_an_empty_field() {
        let keys = parse_extra_keys("golia.ai::/k,:mail:/k,dadaya.jp:mail:", always("PEM"));

        assert!(keys.is_empty());
    }

    #[test]
    fn skips_an_entry_whose_key_file_is_unreadable() {
        let keys = parse_extra_keys("golia.ai:mail:/missing,dadaya.jp:mail:/ok", |path| {
            if path == "/missing" {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
            }
            Ok("PEM".into())
        });

        assert_eq!(keys.len(), 1);
        assert!(keys.contains_key("dadaya.jp"));
    }

    #[test]
    fn empty_input_is_single_domain_mode() {
        assert!(parse_extra_keys("", always("PEM")).is_empty());
        assert!(parse_extra_keys("   ", always("PEM")).is_empty());
    }
}
