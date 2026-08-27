#![deny(missing_docs)]
//! Signals that a message is a fraud attempt rather than mail.
//!
//! ## Why this is its own crate
//!
//! Authentication answers a different question. SPF, DKIM and DMARC say
//! the message really came from the domain in its `From:` header — and
//! a fraudster who registers `auto360d.com` on Tuesday gets all three
//! for free. Measured on one production server: of 25 messages
//! impersonating the company that runs it, **every one passed SPF** and
//! 18 passed DKIM and DMARC as well.
//!
//! What is left is the message itself: who it claims to be from, what
//! wrote it, what it asks for. That is what lives here.
//!
//! ## The shape of every signal in this crate
//!
//! 1. **Measured against a real corpus before it is written**, and the
//!    number goes in the doc comment. A signal whose false-positive
//!    rate nobody has counted is a guess with a function around it.
//! 2. **Scored, not ruled on** — except where the measurement earns a
//!    verdict. [`GENERATED_MAILER_SCORE`] is 5.0 against a 5.0 default
//!    threshold because it matched 29 messages and all 29 were the
//!    fraud; [`CLAIMS_OUR_NAME_SCORE`] is 4.5 because three of its
//!    twelve were Slack.
//! 3. **Its limit is written down.** A fingerprint of one tool stops
//!    working when the tool changes, and says nothing when it does.
//!
//! ## Adding one
//!
//! A module with the check and its tests, a field on [`Findings`], a
//! line in [`scan`] and one in [`score`]. The aggregate is what keeps
//! callers from growing a boolean per signal — `mailrs-inbound` carries
//! one `Findings` and does not change shape when this crate learns
//! something new.
//!
//! ## What is deliberately not here
//!
//! - **Content classification.** Whether the words are spam is
//!   `mailrs-bayes`, which learns; these are structural facts, which do
//!   not.
//! - **Deceptive characters** in a name — `mailrs-textguard`, older and
//!   about typography rather than intent.
//! - **Reputation.** Nothing here remembers a sender. The fraud this
//!   was built against rotates its domain every few messages, so a
//!   memory of domains is a memory of the last wave.

pub mod impersonation;
pub mod mailer_fingerprint;

pub use impersonation::CLAIMS_OUR_NAME_SCORE;
pub use mailer_fingerprint::GENERATED_MAILER_SCORE;

/// What the receiving organisation is called, and who is allowed to say
/// so.
///
/// Empty by default in every field, which turns the checks that need it
/// off: a deployment that has not said what it is called cannot have
/// its name claimed.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// The organisation's own names, as a reader sees them in a display
    /// name — `GOLIA株式会社`, `GOLIA K.K.`.
    ///
    /// **Full names.** Measured on one corpus, the substring `golia`
    /// matched 534 messages of which 490 were GitHub notifications
    /// carrying a `goliajp/…` repository name.
    pub org_names: Vec<String>,
    /// The domains this organisation actually sends from.
    pub our_domains: Vec<String>,
    /// Domains allowed to carry the organisation's name in a display
    /// name — Slack, GitHub, Atlassian and the like, whose
    /// notifications say your company's name because you told them to.
    pub allowed_domains: Vec<String>,
}

/// What a message was found to be doing.
///
/// Every field is a fact about this message alone: no history, no
/// reputation, nothing that has to be kept between messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Findings {
    /// The `From:` display name claims the receiving organisation while
    /// the address is somewhere else.
    pub claims_our_name: bool,
    /// `X-Mailer` is the dotted-number gibberish one bulk tool writes.
    pub generated_mailer: bool,
}

impl Findings {
    /// Whether anything was found at all.
    #[must_use]
    pub fn any(self) -> bool {
        self.claims_our_name || self.generated_mailer
    }
}

/// Run every check over one message's headers.
///
/// `from` is the **decoded** `From:` value, display name and address
/// together; `x_mailer` the raw header value if the message carries
/// one. Undecoded input is the way to make this always answer nothing —
/// the names arrive base64'd inside `=?UTF-8?B?…?=` in every real
/// sample.
#[must_use]
pub fn scan(from: &str, x_mailer: Option<&str>, policy: &Policy) -> Findings {
    Findings {
        claims_our_name: impersonation::claims_our_name(
            from,
            &policy.org_names,
            &policy.our_domains,
            &policy.allowed_domains,
        ),
        generated_mailer: x_mailer.is_some_and(mailer_fingerprint::is_generated_mailer),
    }
}

/// What these findings contribute to a spam total.
///
/// Additive, so two weak signals can reach a threshold neither reaches
/// alone — which is the shape the real messages have.
#[must_use]
pub fn score(findings: Findings) -> f64 {
    let mut total = 0.0;
    if findings.claims_our_name {
        total += CLAIMS_OUR_NAME_SCORE;
    }
    if findings.generated_mailer {
        total += GENERATED_MAILER_SCORE;
    }
    total
}

/// Short names for what fired, for a log line somebody has to read when
/// a message they wanted lands in Junk.
#[must_use]
pub fn reasons(findings: Findings) -> Vec<&'static str> {
    let mut out = Vec::new();
    if findings.claims_our_name {
        out.push("from=claims-our-name");
    }
    if findings.generated_mailer {
        out.push("x-mailer=generated");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            org_names: vec!["GOLIA株式会社".into()],
            our_domains: vec!["golia.jp".into(), "golia.ai".into()],
            allowed_domains: vec!["slack.com".into(), "github.com".into()],
        }
    }

    #[test]
    fn a_clean_message_finds_nothing_and_scores_nothing() {
        let f = scan(
            "Alice <alice@example.com>",
            Some("Microsoft Outlook 16.0"),
            &policy(),
        );
        assert_eq!(f, Findings::default());
        assert!(!f.any());
        assert_eq!(score(f), 0.0);
        assert!(reasons(f).is_empty());
    }

    /// One of the real ones, both signals at once.
    #[test]
    fn the_wave_this_was_built_against() {
        let f = scan(
            "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
            Some("phevb tmiyui 191.8187.55074.84700.25732"),
            &policy(),
        );
        assert!(f.claims_our_name && f.generated_mailer);
        assert_eq!(score(f), CLAIMS_OUR_NAME_SCORE + GENERATED_MAILER_SCORE);
        assert_eq!(reasons(f), ["from=claims-our-name", "x-mailer=generated"]);
    }

    /// A message with no `X-Mailer` at all is the common case and must
    /// not be treated as a generated one.
    #[test]
    fn an_absent_mailer_is_not_a_generated_one() {
        let f = scan("Alice <alice@example.com>", None, &policy());
        assert!(!f.generated_mailer);
    }

    /// An empty policy turns off the checks that need one, and leaves
    /// the ones that do not.
    #[test]
    fn an_empty_policy_still_reads_the_mailer() {
        let f = scan(
            "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
            Some("phevb tmiyui 191.8187.55074.84700.25732"),
            &Policy::default(),
        );
        assert!(
            !f.claims_our_name,
            "an unnamed organisation cannot be claimed"
        );
        assert!(
            f.generated_mailer,
            "the mailer check needs no configuration"
        );
    }
}
