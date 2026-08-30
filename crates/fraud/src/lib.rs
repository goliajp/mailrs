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

pub mod attachment;
pub mod brand;
pub mod finding;
pub mod greeting;
pub mod hostname_claim;
pub mod impersonation;
pub mod mailer_fingerprint;
pub mod minted_address;
pub mod reply_rotation;
pub mod sending_host;

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
    pub allowed_domains: Vec<String>,
}

/// Everything one message offers the rules, extracted once.
///
/// Rules do not parse. The host reads the message and fills this in,
/// because extraction has already gone wrong three separate ways
/// when three code paths each did their own: the receiver, the sweep
/// and the ingest read the `From` differently, and a folded
/// `Message-ID` was invisible to one of them for a day.
///
/// Borrowed throughout — this is built per message on a path that
/// handles every message.
#[derive(Debug, Clone, Default)]
pub struct Facts<'a> {
    /// The decoded `From:` — display name and address together, as
    /// `mailrs_inbound::identity` produces it. Undecoded input is the
    /// way to make every name check answer false: the names arrive
    /// base64'd inside `=?UTF-8?B?…?=` in every real sample.
    pub from: &'a str,
    /// The sending host, lowercased: `mail02.marriottanji.com`.
    pub domain: &'a str,
    /// …and its registrable domain: `marriottanji.com`.
    pub registrable: &'a str,
    /// Messages this deployment has ever had from `registrable`,
    /// **not counting this one**. Zero for a domain nothing has ever
    /// arrived from — which is what makes a brand claim from it
    /// suspicious.
    pub domain_seen: u64,
    /// `X-Mailer`, unfolded, when the message carries one.
    pub x_mailer: Option<&'a str>,
    /// The decoded subject.
    pub subject: &'a str,
    /// RFC 8601 tokens as the receiver recorded them: `pass`,
    /// `fail`, `none`, … Empty when nothing was checked, which is a
    /// different fact from `none`.
    pub spf: &'a str,
    /// DKIM's verdict, aggregated across signatures.
    pub dkim: &'a str,
    /// DMARC's verdict — the one that decides alignment.
    pub dmarc: &'a str,
    /// Zero-width characters in the identifying text that nothing
    /// justifies — `mailrs_textguard`'s reading.
    pub has_zero_width: bool,
    /// A zero-width character inside the **display name**.
    ///
    /// Narrower than `has_zero_width`, which also covers the subject
    /// — and the difference is the whole rule. See [`scan`].
    pub has_zero_width_in_name: bool,
    /// An attachment whose extension the operating system would
    /// execute: `.exe`, `.cab`, `.js`, `.lnk`, `.docm`, and the rest
    /// of that family.
    pub has_executable_attachment: bool,
    /// The `To:` header's display name, decoded.
    ///
    /// Compared against a name the subject greets. Deliberately the
    /// message's *own* claim about who it is for rather than a list
    /// of the reader's names: a list can never be complete, and an
    /// incomplete one convicts every sender who greets the reader by
    /// a name it happens to be missing. Production's account row says
    /// `LI HAO`, the reader is also 李好, and LinkedIn greets 李好 by
    /// name 189 times.
    pub to_display: &'a str,
    /// How many distinct registrable domains have sent mail to this
    /// message's off-domain `Reply-To`, this one included.
    ///
    /// Zero when there is no off-domain reply address, or when the
    /// deployment has no history to count — which reads as *not
    /// rotating*, so a fresh install delivers rather than holds.
    pub reply_rotation: u32,
    /// A bidi override or isolate in the identifying text.
    ///
    /// `mailrs_textguard`'s reading again, and its own note on the
    /// field is the whole argument: *"no legitimate use in a
    /// sender's name."*
    pub has_bidi_override: bool,
}

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
    if impersonation::claims_our_name(
        facts.from,
        &policy.org_names,
        &policy.our_domains,
        &policy.allowed_domains,
    ) {
        out.push(Finding::new(
            RULE_CLAIMS_OUR_NAME,
            Layer::Identity,
            CLAIMS_OUR_NAME_SCORE,
            "display name claims this organisation",
        ));
    }
    if facts
        .x_mailer
        .is_some_and(mailer_fingerprint::is_generated_mailer)
    {
        out.push(Finding::new(
            RULE_GENERATED_MAILER,
            Layer::Provenance,
            GENERATED_MAILER_SCORE,
            "X-Mailer is one no mail client writes",
        ));
    }
    // A display name whose characters are reordered as they render.
    //
    // The one this was written for renders as `iCloud+` and contains
    // no such word. Its display name is, codepoint by codepoint:
    //
    //     U+2066  LEFT-TO-RIGHT ISOLATE
    //     i  C
    //     U+202E  RIGHT-TO-LEFT OVERRIDE
    //     +  d  u  o  l
    //     U+202E  U+2069
    //
    // `+duol` draws backwards as `loud+`, so the screen says
    // `iCloud+`. Every text comparison in this crate looked at a
    // string that does not contain `icloud`, and none of them could
    // have. (rustc rejects those codepoints in a comment for exactly
    // this reason, which is why they are spelled out here.)
    //
    // So the rule is not about which brand is being claimed. It is
    // that the name is built to read as something other than what it
    // is, which is a statement about the sender's intent and needs no
    // list to keep up to date.
    // Zero-width characters spliced between the letters of a name.
    //
    // `mailrs_textguard` scores these toward Junk and notes that one
    // production message in forty carried one legitimately — but that
    // reading covers the **subject** as well. Measured over 35,962
    // messages, forty carry one inside the **display name** and every
    // one of the forty is a phish: `M\u{200c}y\u{200d}JCB`,
    // `Ama\u{200c}zon.co.jp (自動送信メール)`, `AEON\u{200d}`,
    // `三\u{200c}井住\u{200b}友カー\u{feff}ド`. Nobody puts an
    // invisible character in the middle of their own company's name
    // by accident.
    if facts.has_zero_width_in_name {
        out.push(Finding::new(
            RULE_ZERO_WIDTH_NAME,
            Layer::Identity,
            ZERO_WIDTH_NAME_SCORE,
            "the display name has invisible characters spliced into it",
        ));
    }
    // An attachment the operating system will run.
    //
    // One in 35,962: `RFQ-5086-26 TENDER.cab`, 139 KB, SPF fail, sent
    // as a quotation request. Zero false positives in the corpus —
    // the other 1,267 attachments are PDFs, images, spreadsheets and
    // DMARC report gzips.
    //
    // This mailbox's threat is phishing rather than malware, and this
    // is the one delivery vector that was tried. A rule that fires
    // once a year and is right when it does is worth its line.
    if facts.has_executable_attachment {
        out.push(Finding::new(
            RULE_EXECUTABLE_ATTACHMENT,
            Layer::Content,
            EXECUTABLE_ATTACHMENT_SCORE,
            "carries an attachment the operating system would run",
        ));
    }
    if facts.has_bidi_override {
        out.push(Finding::new(
            RULE_BIDI_DISPLAY_NAME,
            Layer::Identity,
            BIDI_DISPLAY_NAME_SCORE,
            "the display name is reordered as it renders — it shows one \
             thing and says another",
        ));
    }
    // Mail that opens by greeting somebody else.
    //
    // **Held.** A sender greeting you by name claims to know who you
    // are; when the name is not yours the claim is false on its face,
    // and it says how the address was obtained — from a list where a
    // stranger's name sat beside it. A reader can check it in a
    // second: *this is addressed to 兰静思, and you are not.*
    if let Some(name) = greeting::greets_someone_else(facts.subject, facts.to_display) {
        out.push(Finding::scored(
            RULE_GREETS_A_STRANGER,
            Layer::Identity,
            GREETS_A_STRANGER_SCORE,
            format!("subject greets `{name}`, which the `To:` header does not name"),
        ));
    }
    // An address whose mailbox and domain were both generated.
    if minted_address::address_looks_minted(facts.from) {
        out.push(Finding::new(
            RULE_MINTED_ADDRESS,
            Layer::Identity,
            MINTED_ADDRESS_SCORE,
            "neither the mailbox nor the domain is a word anybody chose",
        ));
    }
    // One reply address collecting disposable sending domains.
    if reply_rotation::rotates(facts.reply_rotation) {
        out.push(Finding::new(
            RULE_REPLY_DOMAIN_ROTATION,
            Layer::Provenance,
            REPLY_ROTATION_SCORE,
            format!(
                "replies go to a domain that {} different sending domains use",
                facts.reply_rotation
            ),
        ));
    }
    // A hostname that names a company it is not.
    //
    // **Held.** A subdomain is chosen by whoever owns the parent, so
    // `aliyun.rvezovp.cn` is a statement `rvezovp.cn` made about
    // itself and it is false. Not a coincidence of wording, not a
    // habit legitimate senders share — a lie in the envelope, and one
    // the reader can be shown.
    if let Some(company) = hostname_claim::hostname_claims_a_company(facts.domain) {
        out.push(Finding::new(
            RULE_HOSTNAME_CLAIMS_COMPANY,
            Layer::Identity,
            HOSTNAME_CLAIM_SCORE,
            format!("the sending host puts `{company}` in front of a domain that is not theirs"),
        ));
    }
    // A relay host whose name was minted rather than chosen.
    //
    // **Scored, never held.** 34 of the 36 senders it matches in the
    // corpus are phishing — a strong prior, and not a fact about
    // intent: Constant Contact numbers its relays for the same
    // operational reason a phisher does. A legitimate bulk sender
    // must not be hidden for running a mailing list.
    if sending_host::host_label_looks_minted(facts.domain) {
        out.push(Finding::scored(
            RULE_MINTED_SENDING_HOST,
            Layer::Provenance,
            MINTED_SENDING_HOST_SCORE,
            "the sending host's name was generated, not chosen",
        ));
    }
    // **Scored, not held.** See `brand` for why: this rule's second
    // half is not a property of the message at all.
    if brand::subject_claims_brand(facts.subject, facts.domain, facts.domain_seen) {
        out.push(Finding::scored(
            RULE_SUBJECT_CLAIMS_BRAND,
            Layer::Identity,
            brand::IMPERSONATES_BRAND_SCORE,
            "the subject claims a company, from a domain that is not \
             theirs and is new here",
        ));
    }
    if brand::impersonates_brand(facts.from, brand::BRANDS, facts.domain_seen) {
        out.push(Finding::scored(
            RULE_IMPERSONATES_BRAND,
            Layer::Identity,
            brand::IMPERSONATES_BRAND_SCORE,
            "display name claims a company, from a domain that is not \
             theirs and is new here",
        ));
    }
    out
}

/// Somebody claiming to be this organisation.
///
/// A rule id is a wire contract: a stored verdict names it, a
/// release records it, and the per-rule release rate — the only
/// honest false-positive measure there is — counts it. Renaming one
/// is renaming a column.
pub const RULE_CLAIMS_OUR_NAME: &str = "claims-our-name";
/// An `X-Mailer` no mail client writes. See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_GENERATED_MAILER: &str = "x-mailer-generated";
/// Somebody claiming to be a company the reader has an account with.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_IMPERSONATES_BRAND: &str = "impersonates-brand";
/// A subject claiming a company the reader has an account with.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_SUBJECT_CLAIMS_BRAND: &str = "subject-claims-brand";
/// Mail that opens by greeting somebody who is not the reader.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_GREETS_A_STRANGER: &str = "greets-a-stranger";

/// Score for a subject greeting a name the `To:` header does not
/// carry.
///
/// **Suspicion, not a hold.** 146 in the corpus, of which the great
/// majority are the fortune-telling campaign — but a dozen are
/// LinkedIn subjects opening with a Chinese phrase that is not a
/// name at all, and a handful are ordinary senders who greet in the
/// subject and leave the `To:` display name empty. Enough to push
/// toward Junk; not enough to hide mail.
pub const GREETS_A_STRANGER_SCORE: f64 = 2.0;

/// An address whose local part and registered domain both read as
/// machine-minted. See [`minted_address`].
pub const RULE_MINTED_ADDRESS: &str = "minted-address";

/// Score for an address generated on both sides of the `@`.
///
/// Held. 11 of 35,575 production messages, and all eleven are a BEC
/// campaign impersonating this company or Japanese brand phishing.
pub const MINTED_ADDRESS_SCORE: f64 = 6.0;

/// One reply address that many disposable sending domains funnel
/// into. See [`reply_rotation`].
pub const RULE_REPLY_DOMAIN_ROTATION: &str = "reply-domain-rotation";

/// Score for reply-address domain rotation.
///
/// Held on its own. Measured at 258 messages across three campaigns
/// with nothing legitimate above the threshold.
pub const REPLY_ROTATION_SCORE: f64 = 6.0;

/// A hostname naming a company that does not own it.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_HOSTNAME_CLAIMS_COMPANY: &str = "hostname-claims-company";

/// Score for a hostname that borrows a company's name.
///
/// The heaviest of the name signals. The others weigh what a message
/// says about itself, where wording can coincide; this weighs a
/// label somebody registered, where it cannot.
pub const HOSTNAME_CLAIM_SCORE: f64 = 5.0;

/// A relay host whose leading label was minted rather than chosen.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_MINTED_SENDING_HOST: &str = "minted-sending-host";

/// Score for a sending host whose name was generated.
///
/// Below the Junk threshold on its own. 34 of 36 is a prior worth
/// acting on beside anything else and not worth acting on alone —
/// which is exactly what a score buys.
pub const MINTED_SENDING_HOST_SCORE: f64 = 3.0;

/// A display name that renders as something other than what it says.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_BIDI_DISPLAY_NAME: &str = "bidi-display-name";

/// A display name with invisible characters spliced into it.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_ZERO_WIDTH_NAME: &str = "zero-width-name";
/// An attachment the operating system would run.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_EXECUTABLE_ATTACHMENT: &str = "executable-attachment";

/// Score for invisible characters inside a display name.
///
/// Forty in the corpus, forty phishing. Weighted like the other name
/// signals rather than higher: the measurement is of one mailbox's
/// mail, and a name is a weaker thing to convict on than a file the
/// machine would run.
pub const ZERO_WIDTH_NAME_SCORE: f64 = 4.5;

/// Score for an attachment the operating system would run.
///
/// The highest of them. One in 35,962 messages carried one and it
/// was malware; the cost of holding a legitimate one is a click, and
/// the cost of delivering a real one is the machine.
pub const EXECUTABLE_ATTACHMENT_SCORE: f64 = 6.0;

/// Score for a display name carrying a bidi override.
///
/// As high as the two name checks, and for a stronger reason than
/// either: those weigh a claim, and a claim can be innocent. This
/// weighs a mechanism, and `mailrs_textguard`'s own note on the field
/// is that there is *"no legitimate use in a sender's name."* A name
/// that has to be reordered to read correctly was built to be
/// misread.
pub const BIDI_DISPLAY_NAME_SCORE: f64 = 4.5;

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
