//! Why a message was judged the way it was, as a code and its values.
//!
//! A reason is stored once, when the message arrives, and read in
//! whatever language the reader uses. So it carries no sentence: the
//! code names the rule or decision, the params carry the values the
//! sentence needs, and every client turns the code into words from one
//! table, `reasons.json`. A code with no entry in that table is a test
//! failure, not a blank line on a screen.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The text table every client renders reasons from: code → language →
/// sentence, with `{name}` standing for a param.
pub const TABLE: &str = include_str!("reasons.json");

/// One reason: a code from [`TABLE`] and the values its sentence names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reason {
    /// A fraud rule's id (`claims-our-name`), or one of the codes below.
    pub code: String,
    /// The values the sentence names, already formatted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl Reason {
    /// A reason with no values.
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            params: BTreeMap::new(),
        }
    }

    /// The same reason with one more value.
    #[must_use]
    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.params.insert(name.to_string(), value.into());
        self
    }

    /// The same reason with a score, to one decimal place.
    #[must_use]
    pub fn with_score(self, name: &str, value: f64) -> Self {
        self.with(name, format!("{value:.1}"))
    }
}

/// The receiver put it in Junk: it claims one of our domains and fails DMARC.
pub const JUNK_CLAIMS_OUR_DOMAIN: &str = "junk-claims-our-domain";
/// The receiver put it in Junk: the recipient blocked the sender.
pub const JUNK_YOUR_BLOCKLIST: &str = "junk-your-blocklist";
/// The receiver put it in Junk: the sender's domain asks for it (`p=quarantine`).
pub const JUNK_DMARC_QUARANTINE: &str = "junk-dmarc-quarantine";
/// The receiver put it in Junk: the scores added up past the threshold.
pub const JUNK_SCORE: &str = "junk-score";
/// The recipient's own Sieve filter filed it in Junk.
pub const JUNK_YOUR_FILTER: &str = "junk-your-filter";
/// The recipient's rule for this sender chose its category.
pub const SORTED_BY_YOUR_RULE: &str = "sorted-by-your-rule";
/// The classifier trained on the recipient's own sorting chose its category.
pub const SORTED_BY_CLASSIFIER: &str = "sorted-by-classifier";
/// Zero-width characters padding the name or subject.
pub const ZERO_WIDTH_PADDING: &str = "zero-width-padding";
/// The authentication results fold to a suspicious sender.
pub const SENDER_SUSPICIOUS: &str = "sender-suspicious";
/// The content layer's scores.
pub const CONTENT_SCORE: &str = "content-score";
/// One check in the sender's authentication: `check` is `SPF`, `DKIM` or
/// `DMARC`, `result` what it said.
pub const AUTH_CHECK: &str = "auth-check";
/// The name or subject renders reordered (a right-to-left override).
pub const SENDER_BIDI: &str = "sender-bidi";
/// A link whose text shows one host while it goes to another.
pub const LINK_SHOWS_ANOTHER_HOST: &str = "link-shows-another-host";
/// A link to a host on the reputation list.
pub const LINK_HOST_LISTED: &str = "link-host-listed";

/// Every code this crate and its callers produce, besides fraud rule ids.
pub const HOST_CODES: [&str; 14] = [
    JUNK_CLAIMS_OUR_DOMAIN,
    JUNK_YOUR_BLOCKLIST,
    JUNK_DMARC_QUARANTINE,
    JUNK_SCORE,
    JUNK_YOUR_FILTER,
    SORTED_BY_YOUR_RULE,
    SORTED_BY_CLASSIFIER,
    ZERO_WIDTH_PADDING,
    SENDER_SUSPICIOUS,
    CONTENT_SCORE,
    AUTH_CHECK,
    SENDER_BIDI,
    LINK_SHOWS_ANOTHER_HOST,
    LINK_HOST_LISTED,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str(TABLE).expect("reasons.json is a JSON object")
    }

    #[test]
    fn every_host_code_has_a_sentence_in_every_language() {
        let t = table();
        for code in HOST_CODES {
            for lang in ["en", "zh", "ja"] {
                let s = t[code][lang].as_str();
                assert!(
                    s.is_some_and(|s| !s.is_empty()),
                    "{code} has no {lang} text"
                );
            }
        }
    }

    /// A placeholder the sentence names but no producer sets would print
    /// as `{name}`; one the table spells differently in one language
    /// would print in two and not the third.
    #[test]
    fn every_language_names_the_same_params() {
        for (code, langs) in table() {
            let names = |s: &str| {
                let mut v: Vec<String> = s
                    .split('{')
                    .skip(1)
                    .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
                    .collect();
                v.sort();
                v
            };
            let en = names(langs["en"].as_str().unwrap_or(""));
            let with = names(langs["params"]["en"].as_str().unwrap_or(""));
            for lang in ["zh", "ja"] {
                assert_eq!(
                    names(langs[lang].as_str().unwrap_or("")),
                    en,
                    "{code}: {lang} names different params from en"
                );
                assert_eq!(
                    names(langs["params"][lang].as_str().unwrap_or("")),
                    with,
                    "{code}: {lang} names different params from en in the sentence with params"
                );
            }
            if langs.get("params").is_some() {
                assert!(
                    !with.is_empty(),
                    "{code}: a sentence with params names none"
                );
            }
        }
    }

    /// Every rule the shipped rule files declare is a code a verdict can
    /// carry, so each needs its sentence before it ships.
    #[test]
    fn every_shipped_fraud_rule_has_a_sentence_in_every_language() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../fraud-lua/rules");
        let t = table();
        let mut seen = 0;
        for entry in std::fs::read_dir(dir).expect("rule directory") {
            let source = std::fs::read_to_string(entry.expect("entry").path()).expect("rule file");
            for line in source.lines() {
                let Some(rest) = line.strip_prefix("rule('") else {
                    continue;
                };
                let id = rest.split('\'').next().expect("quoted id");
                seen += 1;
                for lang in ["en", "zh", "ja"] {
                    assert!(
                        t.get(id).and_then(|l| l[lang].as_str()).is_some(),
                        "rule {id} has no {lang} text in reasons.json"
                    );
                }
            }
        }
        assert!(seen > 30, "read {seen} rules; the rule files moved");
    }

    /// Every client words reasons from its own copy of the table; a copy
    /// that drifted would show a stale sentence or none.
    #[test]
    fn the_client_copies_of_the_table_are_the_same_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let copy = "web/src/lib/reasons.json";
        let text = std::fs::read_to_string(root.join(copy)).expect(copy);
        assert_eq!(
            text, TABLE,
            "{copy} differs from crates/inbound/src/reasons.json"
        );
    }

    #[test]
    fn a_reason_without_params_serialises_as_its_code_alone() {
        let json = serde_json::to_string(&Reason::new("claims-our-name")).unwrap();
        assert_eq!(json, r#"{"code":"claims-our-name"}"#);
        let back: Reason = serde_json::from_str(&json).unwrap();
        assert!(back.params.is_empty());
    }
}
