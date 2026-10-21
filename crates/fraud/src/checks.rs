//! The compiled checks [`crate::scan`] runs, one function per rule, in
//! the order their findings are reported.

use crate::*;

type Check = fn(&Facts<'_>, &Policy) -> Option<Finding>;

pub(crate) const CHECKS: [Check; 16] = [
    claims_our_name,
    generated_mailer,
    zero_width_in_name,
    executable_attachment,
    bidi_display_name,
    greets_a_stranger,
    zero_width_in_word,
    minted_mailbox_and_domain,
    reply_domain_rotation,
    subject_is_only_our_name,
    impersonates_one_of_us,
    hostname_claims_company,
    minted_sending_host,
    subject_claims_brand,
    brand_is_the_name,
    impersonates_brand,
];

fn claims_our_name(facts: &Facts<'_>, policy: &Policy) -> Option<Finding> {
    impersonation::claims_our_name(
        facts.from,
        &policy.org_names,
        &policy.our_domains,
        &policy.allowed_domains,
    )
    .then(|| {
        Finding::new(
            RULE_CLAIMS_OUR_NAME,
            Layer::Identity,
            CLAIMS_OUR_NAME_SCORE,
            "display name claims this organisation",
        )
    })
}

fn generated_mailer(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    facts
        .x_mailer
        .is_some_and(mailer_fingerprint::is_generated_mailer)
        .then(|| {
            Finding::new(
                RULE_GENERATED_MAILER,
                Layer::Provenance,
                GENERATED_MAILER_SCORE,
                "X-Mailer is one no mail client writes",
            )
        })
}

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
fn zero_width_in_name(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    facts.has_zero_width_in_name.then(|| {
        Finding::new(
            RULE_ZERO_WIDTH_NAME,
            Layer::Identity,
            ZERO_WIDTH_NAME_SCORE,
            "the display name has invisible characters spliced into it",
        )
    })
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
fn executable_attachment(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    facts.has_executable_attachment.then(|| {
        Finding::new(
            RULE_EXECUTABLE_ATTACHMENT,
            Layer::Content,
            EXECUTABLE_ATTACHMENT_SCORE,
            "carries an attachment the operating system would run",
        )
    })
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
fn bidi_display_name(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    facts.has_bidi_override.then(|| {
        Finding::new(
            RULE_BIDI_DISPLAY_NAME,
            Layer::Identity,
            BIDI_DISPLAY_NAME_SCORE,
            "the display name is reordered as it renders — it shows one \
             thing and says another",
        )
    })
}

// Mail that opens by greeting somebody else.
//
// **Held.** A sender greeting you by name claims to know who you
// are; when the name is not yours the claim is false on its face,
// and it says how the address was obtained — from a list where a
// stranger's name sat beside it. A reader can check it in a
// second: *this is addressed to 兰静思, and you are not.*
fn greets_a_stranger(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    greeting::greets_someone_else(facts.subject, facts.to_display).map(|name| {
        Finding::scored(
            RULE_GREETS_A_STRANGER,
            Layer::Identity,
            GREETS_A_STRANGER_SCORE,
            format!("subject greets `{name}`, which the `To:` header does not name"),
        )
    })
}

// A brand name split by invisible characters.
fn zero_width_in_word(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    facts.has_zero_width_inside_a_word.then(|| {
        Finding::new(
            RULE_ZERO_WIDTH_IN_WORD,
            Layer::Content,
            ZERO_WIDTH_IN_WORD_SCORE,
            "invisible characters are wedged inside a word, which defeats a \
             filter while rendering unchanged to the reader",
        )
    })
}

// An address whose mailbox and domain were both generated.
fn minted_mailbox_and_domain(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    minted_address::address_looks_minted(facts.from).then(|| {
        Finding::new(
            RULE_MINTED_ADDRESS,
            Layer::Identity,
            MINTED_ADDRESS_SCORE,
            "neither the mailbox nor the domain is a word anybody chose",
        )
    })
}

// One reply address collecting disposable sending domains.
fn reply_domain_rotation(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    reply_rotation::rotates(facts.reply_rotation).then(|| {
        Finding::new(
            RULE_REPLY_DOMAIN_ROTATION,
            Layer::Provenance,
            REPLY_ROTATION_SCORE,
            format!(
                "replies go to a domain that {} different sending domains use",
                facts.reply_rotation
            ),
        )
    })
}

// A subject that is this organisation's name and nothing else.
fn subject_is_only_our_name(facts: &Facts<'_>, policy: &Policy) -> Option<Finding> {
    subject_is_our_name::subject_is_our_name(
        facts.from,
        facts.subject,
        &policy.org_names,
        &policy.our_domains,
        &policy.allowed_domains,
    )
    .then(|| {
        Finding::new(
            RULE_SUBJECT_IS_OUR_NAME,
            Layer::Identity,
            SUBJECT_IS_OUR_NAME_SCORE,
            "the subject is this organisation's name and little else, from \
             a domain that is not ours",
        )
    })
}

// Somebody wearing one of our own people's names.
fn impersonates_one_of_us(facts: &Facts<'_>, policy: &Policy) -> Option<Finding> {
    impersonation::impersonates_one_of_us(
        facts.from,
        &policy.account_names,
        &policy.our_domains,
        &policy.allowed_domains,
    )
    .then(|| {
        Finding::new(
            RULE_IMPERSONATES_ONE_OF_US,
            Layer::Identity,
            IMPERSONATES_ONE_OF_US_SCORE,
            "the display name is one of this deployment's own people, and \
             the domain is not ours",
        )
    })
}

// A hostname that names a company it is not.
//
// **Held.** A subdomain is chosen by whoever owns the parent, so
// `aliyun.rvezovp.cn` is a statement `rvezovp.cn` made about
// itself and it is false. Not a coincidence of wording, not a
// habit legitimate senders share — a lie in the envelope, and one
// the reader can be shown.
fn hostname_claims_company(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    hostname_claim::hostname_claims_a_company(facts.domain).map(|company| {
        Finding::new(
            RULE_HOSTNAME_CLAIMS_COMPANY,
            Layer::Identity,
            HOSTNAME_CLAIM_SCORE,
            format!("the sending host puts `{company}` in front of a domain that is not theirs"),
        )
    })
}

// A relay host whose name was minted rather than chosen.
//
// **Scored, never held.** 34 of the 36 senders it matches in the
// corpus are phishing — a strong prior, and not a fact about
// intent: Constant Contact numbers its relays for the same
// operational reason a phisher does. A legitimate bulk sender
// must not be hidden for running a mailing list.
fn minted_sending_host(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    sending_host::host_label_looks_minted(facts.domain).then(|| {
        Finding::scored(
            RULE_MINTED_SENDING_HOST,
            Layer::Provenance,
            MINTED_SENDING_HOST_SCORE,
            "the sending host's name was generated, not chosen",
        )
    })
}

// **Scored, not held.** See `brand` for why: this rule's second
// half is not a property of the message at all.
fn subject_claims_brand(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    brand::subject_claims_brand(facts.subject, facts.domain, facts.domain_seen).then(|| {
        Finding::scored(
            RULE_SUBJECT_CLAIMS_BRAND,
            Layer::Identity,
            brand::IMPERSONATES_BRAND_SCORE,
            "the subject claims a company, from a domain that is not \
             theirs and is new here",
        )
    })
}

// **Held**, where the substring version beside it is only
// scored. Being named exactly for a company you do not send for
// is a fact about the message; whether we have heard from your
// domain before is a fact about us.
fn brand_is_the_name(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    brand::brand_is_the_display_name(facts.from, brand::BRANDS).then(|| {
        Finding::new(
            RULE_BRAND_IS_THE_NAME,
            Layer::Identity,
            BRAND_IS_THE_NAME_SCORE,
            "the display name is a company's and nothing else, and the \
             domain is not theirs",
        )
    })
}

fn impersonates_brand(facts: &Facts<'_>, _: &Policy) -> Option<Finding> {
    brand::impersonates_brand(facts.from, brand::BRANDS, facts.domain_seen).then(|| {
        Finding::scored(
            RULE_IMPERSONATES_BRAND,
            Layer::Identity,
            brand::IMPERSONATES_BRAND_SCORE,
            "display name claims a company, from a domain that is not \
             theirs and is new here",
        )
    })
}
