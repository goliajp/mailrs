//! What a rule found, as a value rather than a field.
//!
//! # Why this is a list
//!
//! `Findings` was a struct with one `bool` per check. Three checks
//! cost **36 references across seven files** outside this crate, and
//! each new one was a field, a wire change, a line in the verdict's
//! renderer, and a call site in the receiver, the sweep and the
//! ingest — for a rule whose body is three lines. That is a linear
//! cost with a large constant, and the plan is many rules.
//!
//! So a finding is a value. Adding a rule adds an element; nothing
//! above this changes.
//!
//! # Why a rule returns this and not an action
//!
//! A `Finding` names a rule, a layer, a score and a sentence. It
//! cannot name a mailbox, a key, a thread or a file — so a scripted
//! rule has no verb, nothing to inject into, and nothing to delete
//! with. The host reads findings and chooses from a closed set of
//! actions whose ceiling is quarantine.
//!
//! That is the whole of the safety argument for running rules people
//! can edit without a rebuild, and it is structural rather than
//! reviewed.

use std::fmt;

/// Which of the four reviews a finding belongs under.
///
/// The verdict shows these four, always, in this order — a reader
/// comparing two messages is comparing like with like. A rule
/// declares which one it speaks for; the renderer groups by it and
/// never needs editing again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Did it come from where it says? SPF, DKIM, DMARC.
    Transport,
    /// Is the name it shows a claim it can make?
    Identity,
    /// Did a real mail client write it?
    Provenance,
    /// Does it read like the fraud it is?
    Content,
}

impl Layer {
    /// The four, in the order they are always shown.
    pub const ALL: [Layer; 4] = [
        Layer::Transport,
        Layer::Identity,
        Layer::Provenance,
        Layer::Content,
    ];

    /// The name this layer goes by on the wire and on screen.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Transport => "transport",
            Layer::Identity => "identity",
            Layer::Provenance => "provenance",
            Layer::Content => "content",
        }
    }

    /// Parse a layer name, for a rule set written outside Rust.
    ///
    /// `None` for anything else, and the caller must reject the rule
    /// rather than pick a default: a rule filed under the wrong
    /// review tells the reader the wrong story about why their mail
    /// was held, and a silent default is how it would happen.
    #[must_use]
    pub fn parse(s: &str) -> Option<Layer> {
        match s {
            "transport" => Some(Layer::Transport),
            "identity" => Some(Layer::Identity),
            "provenance" => Some(Layer::Provenance),
            "content" => Some(Layer::Content),
            _ => None,
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing one rule found in one message.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable identifier — `claims-our-name`, `x-mailer-generated`,
    /// `impersonates-brand`, or whatever a scripted rule calls
    /// itself.
    ///
    /// It is a wire contract: a stored verdict names it, a release
    /// records it, and the per-rule release rate — the only honest
    /// false-positive measure there is — counts it. Renaming one is
    /// renaming a column.
    pub rule: String,
    /// Which review it speaks for.
    pub layer: Layer,
    /// What it contributes to the spam total.
    ///
    /// The **hold** is not a score decision — see the crate's
    /// `holds` — but the Junk threshold still is, and these two
    /// audiences want different arithmetic.
    pub score: f64,
    /// What it actually saw, in the words a reader needs when asking
    /// why their mail is here. Shown verbatim.
    pub detail: String,
}

impl Finding {
    /// A finding, spelled out.
    pub fn new(
        rule: impl Into<String>,
        layer: Layer,
        score: f64,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            rule: rule.into(),
            layer,
            score,
            detail: detail.into(),
        }
    }
}

/// Everything found in one message, by every rule that looked.
///
/// Order is the order the rules ran, which is the order they are
/// reported in. Nothing depends on it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Findings(Vec<Finding>);

impl Findings {
    /// Nothing found yet.
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Record one more.
    pub fn push(&mut self, f: Finding) {
        self.0.push(f);
    }

    /// Merge another producer's findings in.
    ///
    /// Compiled rules and scripted ones are two producers of one
    /// type; this is where they meet, and neither knows about the
    /// other.
    pub fn extend(&mut self, other: Findings) {
        self.0.extend(other.0);
    }

    /// Whether anything was found at all.
    #[must_use]
    pub fn any(&self) -> bool {
        !self.0.is_empty()
    }

    /// How many rules fired.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether none did. The inverse of [`Findings::any`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The sum of what was found, for the Junk threshold.
    #[must_use]
    pub fn score(&self) -> f64 {
        self.0.iter().map(|f| f.score).sum()
    }

    /// Whether a particular rule fired.
    ///
    /// For the handful of places that ask about one by name. A caller
    /// reaching for this a lot is a caller that wants `iter`.
    #[must_use]
    pub fn has(&self, rule: &str) -> bool {
        self.0.iter().any(|f| f.rule == rule)
    }

    /// Every finding, in the order the rules ran.
    pub fn iter(&self) -> std::slice::Iter<'_, Finding> {
        self.0.iter()
    }

    /// What fired, for the log line somebody reads when a message
    /// they wanted lands in Junk.
    #[must_use]
    pub fn rules(&self) -> Vec<&str> {
        self.0.iter().map(|f| f.rule.as_str()).collect()
    }

    /// The findings filed under one review.
    pub fn in_layer(&self, layer: Layer) -> impl Iterator<Item = &Finding> {
        self.0.iter().filter(move |f| f.layer == layer)
    }
}

impl FromIterator<Finding> for Findings {
    fn from_iter<T: IntoIterator<Item = Finding>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl IntoIterator for Findings {
    type Item = Finding;
    type IntoIter = std::vec::IntoIter<Finding>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Findings for a test, by rule id.
///
/// A test that pokes a struct field is a test that stops asking the
/// question the production path asks the moment the field goes away —
/// and every one of them did. This keeps the shorthand without the
/// second definition: the ids are the crate's own constants and the
/// layers are the ones the rules really file under.
#[cfg(any(test, feature = "testing"))]
#[must_use]
pub fn findings_for(rules: &[&str]) -> Findings {
    rules
        .iter()
        .map(|r| match *r {
            crate::RULE_CLAIMS_OUR_NAME => Finding::new(
                *r,
                Layer::Identity,
                crate::CLAIMS_OUR_NAME_SCORE,
                "display name claims this organisation",
            ),
            crate::RULE_GENERATED_MAILER => Finding::new(
                *r,
                Layer::Provenance,
                crate::GENERATED_MAILER_SCORE,
                "X-Mailer is one no mail client writes",
            ),
            crate::RULE_IMPERSONATES_BRAND => Finding::new(
                *r,
                Layer::Identity,
                crate::brand::IMPERSONATES_BRAND_SCORE,
                "display name claims a company, from a domain that is not theirs and is new here",
            ),
            other => panic!("no such rule: {other}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rule: &str, layer: Layer, score: f64) -> Finding {
        Finding::new(rule, layer, score, "saw something")
    }

    #[test]
    fn nothing_found_is_nothing_scored() {
        let n = Findings::new();
        assert!(!n.any());
        assert_eq!(n.score(), 0.0);
        assert!(n.rules().is_empty());
    }

    #[test]
    fn the_score_is_the_sum_and_the_rules_are_the_names() {
        let mut n = Findings::new();
        n.push(f("a", Layer::Identity, 4.5));
        n.push(f("b", Layer::Provenance, 5.0));
        assert_eq!(n.score(), 9.5);
        assert_eq!(n.rules(), ["a", "b"]);
        assert!(n.has("a") && !n.has("c"));
    }

    /// Two producers, one type — the merge is the only place they
    /// meet, and neither can tell afterwards which was which.
    #[test]
    fn two_producers_merge_into_one_set() {
        let mut compiled = Findings::new();
        compiled.push(f("x-mailer-generated", Layer::Provenance, 5.0));
        let mut scripted = Findings::new();
        scripted.push(f("lua:new-shape", Layer::Content, 2.0));

        compiled.extend(scripted);
        assert_eq!(compiled.len(), 2);
        assert_eq!(compiled.score(), 7.0);
    }

    #[test]
    fn findings_group_by_the_review_they_speak_for() {
        let mut n = Findings::new();
        n.push(f("a", Layer::Identity, 1.0));
        n.push(f("b", Layer::Identity, 2.0));
        n.push(f("c", Layer::Content, 3.0));
        assert_eq!(n.in_layer(Layer::Identity).count(), 2);
        assert_eq!(n.in_layer(Layer::Transport).count(), 0);
    }

    /// A layer name from outside Rust is validated, never defaulted.
    /// A rule filed under the wrong review tells the reader the wrong
    /// story about why their mail was held.
    #[test]
    fn an_unknown_layer_is_rejected_rather_than_defaulted() {
        assert_eq!(Layer::parse("identity"), Some(Layer::Identity));
        assert_eq!(Layer::parse("Identity"), None);
        assert_eq!(Layer::parse("whatever"), None);
        for l in Layer::ALL {
            assert_eq!(Layer::parse(l.as_str()), Some(l));
        }
    }
}
