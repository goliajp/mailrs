//! Per-domain signing keys written as `domain:selector:key_path[,...]`,
//! and the rule that picks which entry signs mail from a given domain.
//!
//! The signer and anything that reports on signing (a DNS check, say)
//! must agree on both, so both live here rather than in either caller.

use std::collections::HashMap;

/// One well-formed entry of a key list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyListEntry<'a> {
    /// Signing domain, lowercased: the `d=` tag.
    pub domain: String,
    /// Selector: the label under `<selector>._domainkey.<domain>`.
    pub selector: &'a str,
    /// Path of the private key file; may itself contain colons.
    pub key_path: &'a str,
}

/// Why an entry was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Fewer than three colon-separated fields.
    NotThreeFields,
    /// One of the three fields is empty.
    EmptyField,
}

/// An entry that was left out, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry<'a> {
    /// The entry text, trimmed.
    pub entry: &'a str,
    /// What was wrong with it.
    pub reason: SkipReason,
}

/// Split a key list into its entries, in order.
///
/// Blank entries (doubled or trailing commas, surrounding space) are
/// dropped silently; anything else that is not `domain:selector:path`
/// comes back as a [`SkippedEntry`] so the caller can say so. Only the
/// first two colons separate fields.
pub fn parse_key_list(raw: &str) -> Vec<Result<KeyListEntry<'_>, SkippedEntry<'_>>> {
    raw.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|entry| {
            let parts: Vec<&str> = entry.splitn(3, ':').map(str::trim).collect();
            let [domain, selector, key_path] = parts.as_slice() else {
                return Err(SkippedEntry {
                    entry,
                    reason: SkipReason::NotThreeFields,
                });
            };
            if domain.is_empty() || selector.is_empty() || key_path.is_empty() {
                return Err(SkippedEntry {
                    entry,
                    reason: SkipReason::EmptyField,
                });
            }
            Ok(KeyListEntry {
                domain: domain.to_ascii_lowercase(),
                selector,
                key_path,
            })
        })
        .collect()
}

/// The entry that signs mail from `from_domain`, with the domain it is
/// keyed under.
///
/// An exact match wins; otherwise the leading label is stripped one at
/// a time (never down to a bare TLD), so `mail.example.com` is signed
/// with `example.com`'s key, which still aligns under relaxed DKIM
/// alignment. `None` means the caller's default key signs.
pub fn entry_for_domain<'k, V>(
    keys: &'k HashMap<String, V>,
    from_domain: &str,
) -> Option<(&'k str, &'k V)> {
    let mut candidate = from_domain;
    loop {
        if let Some((k, v)) = keys.get_key_value(candidate) {
            return Some((k.as_str(), v));
        }
        let (_, parent) = candidate.split_once('.')?;
        if !parent.contains('.') {
            return None;
        }
        candidate = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_reports_the_bad_ones() {
        let got = parse_key_list(" GOLIA.AI : mail : /a:b.key , ,broken,x::/k,dadaya.jp:s1:/k,");
        assert_eq!(
            got,
            vec![
                Ok(KeyListEntry {
                    domain: "golia.ai".into(),
                    selector: "mail",
                    key_path: "/a:b.key",
                }),
                Err(SkippedEntry {
                    entry: "broken",
                    reason: SkipReason::NotThreeFields,
                }),
                Err(SkippedEntry {
                    entry: "x::/k",
                    reason: SkipReason::EmptyField,
                }),
                Ok(KeyListEntry {
                    domain: "dadaya.jp".into(),
                    selector: "s1",
                    key_path: "/k",
                }),
            ]
        );
        assert!(parse_key_list("  ").is_empty());
    }

    #[test]
    fn exact_match_then_parent_never_a_bare_tld() {
        let keys: HashMap<String, u8> = [("example.com".to_string(), 1), ("com".to_string(), 2)]
            .into_iter()
            .collect();
        assert_eq!(
            entry_for_domain(&keys, "example.com"),
            Some(("example.com", &1))
        );
        assert_eq!(
            entry_for_domain(&keys, "a.mail.example.com"),
            Some(("example.com", &1))
        );
        assert_eq!(entry_for_domain(&keys, "other.org"), None);
        assert_eq!(entry_for_domain(&keys, "x.com"), None);
    }
}
