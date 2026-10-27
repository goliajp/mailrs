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
//! check in `checks.rs` (which [`scan`] runs) and a line in
//! [`score`](Findings::score). The aggregate is what keeps
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

pub mod advance_fee;
pub mod attachment;
pub mod brand;
mod checks;
mod facts;
pub mod finding;
pub mod greeting;
pub mod hostname_claim;
pub mod impersonation;
pub mod line_handoff;
pub mod links;
pub mod mailer_fingerprint;
pub mod minted_address;
pub mod reply_rotation;
pub mod sending_host;
pub mod subject_is_our_name;

#[cfg(any(test, feature = "testing"))]
pub use finding::findings_for;
pub use finding::{Finding, Findings, Layer};
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
    ///
    /// They are allowed the *people's* names too, and for the same
    /// reason: `LI HAO <jira@golia.atlassian.net>` is Jira saying who
    /// did the thing.
    pub allowed_domains: Vec<String>,
    /// The display names on this deployment's own account rows.
    ///
    /// Unlike the set of names the *reader* is addressed by, this one
    /// can be complete: a deployment knows who holds an account on
    /// it. Empty turns the check off, which is the safe direction —
    /// see [`impersonation::impersonates_one_of_us`].
    pub account_names: Vec<String>,
}

pub use facts::{ATTACHMENT_TEXT_CAP, Facts};

/// Run every compiled check over one message.
///
/// The scripted rules are a second producer of the same `Findings`;
/// the caller merges. Neither knows about the other, and nothing
/// downstream can tell which produced what — which is the point:
/// a rule earns a compiled home by being measured, not by being
/// special.
#[must_use]
pub fn scan(facts: &Facts<'_>, policy: &Policy) -> Findings {
    let mut out = Findings::new();
    for check in checks::CHECKS {
        if let Some(finding) = check(facts, policy) {
            out.push(finding);
        }
    }
    out
}

pub use ids::*;

mod ids;

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            account_names: Vec::new(),
            org_names: vec!["GOLIA株式会社".into()],
            our_domains: vec!["golia.jp".into(), "golia.ai".into()],
            allowed_domains: vec!["slack.com".into(), "github.com".into()],
        }
    }

    /// The facts of one message, with only what a case is about
    /// filled in.
    fn facts<'a>(from: &'a str, x_mailer: Option<&'a str>, seen: u64) -> Facts<'a> {
        Facts {
            from,
            x_mailer,
            domain_seen: seen,
            ..Facts::default()
        }
    }

    #[test]
    fn a_clean_message_finds_nothing_and_scores_nothing() {
        let f = scan(
            &facts(
                "Alice <alice@example.com>",
                Some("Microsoft Outlook 16.0"),
                0,
            ),
            &policy(),
        );
        assert!(!f.any());
        assert_eq!(f.score(), 0.0);
        assert!(f.rules().is_empty());
    }

    #[test]
    fn reported_chatgpt_payment_phish_is_held_even_with_history_and_auth_pass() {
        // Visible From and subject transcribed from the 2026-09-06 screenshot.
        // Authentication and history below are hypothetical: neither should
        // excuse an unrelated domain claiming to be ChatGPT.
        for seen in [0, 100] {
            let f = Facts {
                from: "ChatGPT <admin@heavenerandassociates.com>",
                domain: "heavenerandassociates.com",
                registrable: "heavenerandassociates.com",
                subject: "[最終リマインダー]: お支払い方法を更新してください。",
                domain_seen: seen,
                spf: "pass",
                dkim: "pass",
                dmarc: "pass",
                ..Facts::default()
            };
            let found = scan(&f, &Policy::default());
            assert!(found.has(RULE_BRAND_IS_THE_NAME));
            assert!(found.hold_worthy());
        }
    }

    /// One of the real ones, both signals at once.
    #[test]
    fn the_wave_this_was_built_against() {
        let f = scan(
            &facts(
                "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
                Some("phevb tmiyui 191.8187.55074.84700.25732"),
                0,
            ),
            &policy(),
        );
        assert!(f.has(RULE_CLAIMS_OUR_NAME) && f.has(RULE_GENERATED_MAILER));
        assert_eq!(f.score(), CLAIMS_OUR_NAME_SCORE + GENERATED_MAILER_SCORE);
    }

    /// Every finding carries the review it speaks for, so the verdict
    /// can group by it instead of naming each rule.
    #[test]
    fn each_finding_is_filed_under_a_review() {
        let f = scan(
            &facts(
                "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
                Some("phevb tmiyui 191.8187.55074.84700.25732"),
                0,
            ),
            &policy(),
        );
        assert_eq!(f.in_layer(Layer::Identity).count(), 1);
        assert_eq!(f.in_layer(Layer::Provenance).count(), 1);
        assert_eq!(f.in_layer(Layer::Transport).count(), 0);
    }

    /// Every finding says what it saw. A rule that fires without a
    /// sentence gives the reader a hold they cannot argue with.
    #[test]
    fn every_finding_explains_itself() {
        let f = scan(
            &facts("iCloud+ <zkxfp@zkxfp.zctxiot.com>", None, 0),
            &policy(),
        );
        assert!(f.any());
        for finding in f.iter() {
            assert!(
                !finding.detail.trim().is_empty(),
                "{} said nothing",
                finding.rule
            );
            assert!(!finding.rule.trim().is_empty());
            assert!(finding.score > 0.0);
        }
    }

    /// The one that got through, byte for byte.
    ///
    /// Its display name renders as `iCloud+` and contains no such
    /// word — `\u{2066}iC\u{202e}+duol\u{202e}\u{2069}`, where the
    /// override draws `+duol` backwards. The brand check compares
    /// text and the text does not say it, so nothing that reads the
    /// name could have caught this. What catches it is the mechanism.
    #[test]
    fn a_name_reordered_as_it_renders_is_caught_without_reading_it() {
        let from = "\u{2066}iC\u{202e}+duol\u{202e}\u{2069} <zkxfp@zkxfp.zctxiot.com>";

        // What the name checks see: not the word, and never will be.
        let folded = impersonation::fold(from);
        assert!(
            !folded.contains("icloud"),
            "the folded name contains the brand after all: {folded:?}"
        );
        let mut without = facts(from, None, 0);
        without.has_bidi_override = false;
        assert!(
            !scan(&without, &policy()).has(RULE_BIDI_DISPLAY_NAME),
            "fired without the deception being present"
        );

        // What catches it.
        let mut with = facts(from, None, 0);
        with.has_bidi_override = true;
        let f = scan(&with, &policy());
        assert!(f.has(RULE_BIDI_DISPLAY_NAME));
        assert_eq!(f.in_layer(Layer::Identity).count(), 1);
    }

    /// Forty in the corpus, forty phishing — and the reason the rule
    /// is about the **display name** rather than the identifying text
    /// as a whole: `mailrs_textguard` measured one legitimate message
    /// in forty carrying a zero-width space, and it was in a subject.
    #[test]
    fn invisible_characters_in_a_name_are_caught() {
        // Real ones, byte for byte.
        for from in [
            "M\u{200c}y\u{200d}JC\u{feff}B <ohgcji@ohgcji.yymtjj.com>",
            "Ama\u{200c}zon.co.jp (自動送信メール) <drink@drink.thinking-progress.com>",
            "AEON\u{200d} <noreply@6eryj.esskkd.com>",
        ] {
            let mut f = facts(from, None, 0);
            f.has_zero_width_in_name = true;
            assert!(
                scan(&f, &policy()).has(RULE_ZERO_WIDTH_NAME),
                "not caught: {from}"
            );
        }
    }

    /// And the negative — the rule is the host's reading, so a name
    /// without one must not fire it.
    #[test]
    fn an_ordinary_name_has_no_invisible_characters() {
        let f = scan(&facts("Alice <alice@example.com>", None, 0), &policy());
        assert!(!f.has(RULE_ZERO_WIDTH_NAME));
    }

    /// The one piece of malware the corpus contains, and the only
    /// executable attachment in 1,268: `RFQ-5086-26 TENDER.cab`, SPF
    /// fail, dressed as a quotation request.
    #[test]
    fn an_attachment_the_machine_would_run_is_the_heaviest_finding() {
        let mut f = facts("Tender <tender@tsanglik.com.hk>", None, 99);
        f.has_executable_attachment = true;
        let found = scan(&f, &policy());
        assert!(found.has(RULE_EXECUTABLE_ATTACHMENT));
        assert_eq!(found.in_layer(Layer::Content).count(), 1);
        // Heavier than any name signal — a name is a claim, a file
        // the machine runs is the machine. Asserted on the finding
        // rather than on the constants, which the compiler settles
        // and a runtime check would only restate.
        let weight = found
            .iter()
            .find(|f| f.rule == RULE_EXECUTABLE_ATTACHMENT)
            .map(|f| f.score)
            .expect("the finding is there");
        assert!(weight > CLAIMS_OUR_NAME_SCORE);
    }

    /// **宁纵勿枉.** Only what the message itself is doing may hide
    /// it.
    ///
    /// A brand claim is a strong prior, but its second half —
    /// "and we have not heard from this domain before" — is a fact
    /// about our own history, equally true of every legitimate
    /// correspondent writing for the first time. It cannot be shown
    /// to a reader as a reason. So it scores, and the reader still
    /// sees the mail in Junk.
    ///
    /// What may hide mail is a characteristic of the message with no
    /// legitimate use: a name reordered as it renders, invisible
    /// characters spliced into one, an `X-Mailer` no client writes,
    /// an attachment the machine would run.
    #[test]
    fn only_an_intrinsic_characteristic_may_hide_mail() {
        // A brand claim and nothing else: scored, visible.
        let mut brandish = facts(
            "Amazon 配送センター <noreply@mail02.marriottanji.com>",
            None,
            0,
        );
        brandish.subject = "【重要】Amazonプライム：支払い方法未更新";
        brandish.domain = "mail02.marriottanji.com";
        let f = scan(&brandish, &policy());
        assert!(f.any(), "the claim is still recorded");
        assert!(f.score() > 0.0, "and still pushes toward Junk");
        assert!(
            !f.hold_worthy(),
            "a claim resting on our own history hid the mail"
        );

        // The same message, now doing something to its own name.
        let mut with_name = brandish.clone();
        with_name.has_zero_width_in_name = true;
        assert!(
            scan(&with_name, &policy()).hold_worthy(),
            "an invisible character spliced into a name is the message's own doing"
        );
    }

    /// The four that may. Each names something the message is doing,
    /// and each was measured right on nearly everything it fired on.
    #[test]
    fn the_intrinsic_signals_are_the_ones_that_hold() {
        let base = || facts("Someone <a@brand-new.example>", None, 0);
        let mut bidi = base();
        bidi.has_bidi_override = true;
        let mut zero_width = base();
        zero_width.has_zero_width_in_name = true;
        let mut executable = base();
        executable.has_executable_attachment = true;
        let mut mailer = base();
        mailer.x_mailer = Some("phevb tmiyui 191.8187.55074.84700.25732");

        for (what, f) in [
            ("a name reordered as it renders", bidi),
            ("invisible characters in a name", zero_width),
            ("an attachment the machine runs", executable),
            ("an X-Mailer no client writes", mailer),
        ] {
            assert!(
                scan(&f, &policy()).hold_worthy(),
                "{what} did not earn a hold"
            );
        }
    }

    /// A message with no `X-Mailer` at all is the common case and must
    /// not be treated as a generated one.
    #[test]
    fn an_absent_mailer_is_not_a_generated_one() {
        let f = scan(&facts("Alice <alice@example.com>", None, 0), &policy());
        assert!(!f.has(RULE_GENERATED_MAILER));
    }

    /// An empty policy turns off the checks that need one, and leaves
    /// the ones that do not.
    #[test]
    fn an_empty_policy_still_reads_the_mailer() {
        let f = scan(
            &facts(
                "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
                Some("phevb tmiyui 191.8187.55074.84700.25732"),
                0,
            ),
            &Policy::default(),
        );
        assert!(
            !f.has(RULE_CLAIMS_OUR_NAME),
            "an unnamed organisation cannot be claimed"
        );
        assert!(
            f.has(RULE_GENERATED_MAILER),
            "the mailer check needs no configuration"
        );
    }
}
