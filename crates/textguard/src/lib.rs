//! Characters that deceive a reader about who sent a message.
//!
//! Not "invisible characters" — that phrase is what makes this kind of
//! check wrong. Three of the four invisible classes have real
//! typographic work to do, and 59 of 33,602 production messages carry
//! them: zero-width joiner builds emoji sequences and Indic conjuncts,
//! zero-width non-joiner is *required* for correct word shaping in
//! Persian and Hindi, the byte-order mark is a byte-order mark, and the
//! soft hyphen is a hyphenation hint. Rejecting those means telling
//! Persian and Hindi senders their mail looks forged.
//!
//! So this names the codepoints it rejects, one at a time, and says why
//! each has no other use.
//!
//! # What it is for
//!
//! Measured over the same 33,602 messages, on From display names and
//! Subjects only:
//!
//! | class | messages | of which phishing |
//! |---|---:|---|
//! | bidi overrides | 5 (0.015%) | **5 — no false positives** |
//! | unjustified zero-width | 40 (0.119%) | 39 |
//! | legitimate invisibles | 59 (0.176%) | must never be flagged |
//!
//! The five bidi ones read, once the override does its work:
//!
//! ```text
//!   <RLO>DRAC NOSIAS   →  SAISON CARD
//!   <RLO>BCJyM         →  MyJCB
//!   <LRI><RLO>【 pj.oc.nozamA 】<PDF><PDI>  →  Amazon.co.jp
//! ```
//!
//! Written with the controls spelled out rather than pasted: `rustc`
//! refuses a source file containing a codepoint that changes the visible
//! direction of text, which it has since CVE-2021-42574 ("Trojan
//! Source"). The compiler taking the same position as this module is a
//! reasonable second opinion on the premise.
//!
//! Two properties make this worth having beside a content classifier.
//! It is **language- and topic-independent**: it does not ask what the
//! mail says, it asks whether somebody tampered at the character layer,
//! which is a statement about intent. And it needs **no brand list** —
//! nothing has to know that JCB, SAISON, 楽天 and Amazon are worth
//! impersonating.
//!
//! # What it is not for
//!
//! Bodies. An invisible character in a body has innocent sources —
//! pasted text, a tracking pixel's alt text, a CSS-hidden preheader —
//! and the deception this catches is specifically about *identity as
//! displayed*.
//!
//! Homoglyphs (Cyrillic а for Latin a) are a real and larger problem
//! with real false positives, and they are deliberately not here: they
//! deserve their own measurement rather than being smuggled in beside
//! something that measured clean.

#![forbid(unsafe_code)]

/// A right-to-left or left-to-right **override**, or an isolate that can
/// carry one, in text meant to identify a sender.
///
/// These force a rendering the characters do not imply. Real
/// right-to-left text does not need them: the Unicode bidirectional
/// algorithm derives direction from the characters' own properties, and
/// a Hebrew or Arabic sender's name renders correctly with none of
/// these present. The override exists precisely to make text render as
/// something other than what it is.
///
/// `PDF` and `PDI` (the pops) are included because they only appear to
/// close an embedding or isolate — their presence means one was opened,
/// even if the opener was stripped somewhere upstream.
const BIDI_CONTROLS: &[char] = &[
    '\u{202A}', // LEFT-TO-RIGHT EMBEDDING
    '\u{202B}', // RIGHT-TO-LEFT EMBEDDING
    '\u{202C}', // POP DIRECTIONAL FORMATTING
    '\u{202D}', // LEFT-TO-RIGHT OVERRIDE
    '\u{202E}', // RIGHT-TO-LEFT OVERRIDE  ← all five production hits
    '\u{2066}', // LEFT-TO-RIGHT ISOLATE
    '\u{2067}', // RIGHT-TO-LEFT ISOLATE
    '\u{2068}', // FIRST STRONG ISOLATE
    '\u{2069}', // POP DIRECTIONAL ISOLATE
];

/// Zero-width characters with **no typographic job**, as against the
/// four that have one.
///
/// Each entry needs its own justification, because the whole risk of
/// this check is over-reach:
///
/// * `U+200B` ZERO WIDTH SPACE — a line-break opportunity. Nothing in
///   mail headers wraps, and no script requires it for shaping.
/// * `U+2060` WORD JOINER — the inverse, a break *suppressor*. Same.
/// * `U+180E` MONGOLIAN VOWEL SEPARATOR — reclassified as formatting in
///   Unicode 6.3 and zero-width since; not used in modern Mongolian
///   text.
/// * `U+2061`–`U+2064` — invisible **mathematical** operators (function
///   application, times, separator, plus). They belong in MathML, not in
///   a person's name.
///
/// Deliberately **absent**, and this list is the point of the module:
/// `U+200C` ZWNJ (Persian, Hindi word shaping), `U+200D` ZWJ (emoji
/// sequences, Indic conjuncts), `U+FEFF` (byte-order mark) and `U+00AD`
/// (soft hyphen).
const UNJUSTIFIED_ZERO_WIDTH: &[char] = &[
    '\u{200B}', // ZERO WIDTH SPACE
    '\u{2060}', // WORD JOINER
    '\u{180E}', // MONGOLIAN VOWEL SEPARATOR
    '\u{2061}', // FUNCTION APPLICATION
    '\u{2062}', // INVISIBLE TIMES
    '\u{2063}', // INVISIBLE SEPARATOR
    '\u{2064}', // INVISIBLE PLUS
];

/// What a piece of identifying text was found to contain.
///
/// Two fields rather than one score, because the two carry different
/// weight and the caller has to be able to treat them differently: one
/// measured with no false positives, the other with one in forty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Deception {
    /// A bidi override or isolate. No legitimate use in a sender's name.
    pub bidi_override: bool,
    /// A zero-width character with no typographic job. Suggestive, not
    /// conclusive — one production message in forty was a real newsletter
    /// with a zero-width space inside a long subject.
    pub unjustified_zero_width: bool,
}

impl Deception {
    /// Whether anything at all was found.
    pub fn any(self) -> bool {
        self.bidi_override || self.unjustified_zero_width
    }
}

/// Examine text that identifies a sender — a From display name, a
/// Subject — for characters placed there to deceive.
///
/// Pass the **decoded** text: RFC 2047 encoded-words must be decoded
/// first, or the deception is hidden inside base64 and this sees only
/// `=?UTF-8?B?…?=`.
pub fn deception_in(text: &str) -> Deception {
    let mut out = Deception::default();
    for c in text.chars() {
        if BIDI_CONTROLS.contains(&c) {
            out.bidi_override = true;
        } else if UNJUSTIFIED_ZERO_WIDTH.contains(&c) {
            out.unjustified_zero_width = true;
        }
        if out.bidi_override && out.unjustified_zero_width {
            break;
        }
    }
    out
}

/// The same over several fields — a From display name and a Subject,
/// typically — folded into one verdict.
pub fn deception_in_any<'a>(texts: impl IntoIterator<Item = &'a str>) -> Deception {
    let mut out = Deception::default();
    for t in texts {
        let d = deception_in(t);
        out.bidi_override |= d.bidi_override;
        out.unjustified_zero_width |= d.unjustified_zero_width;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five production messages, verbatim. Each is a real brand name
    /// written backwards behind a right-to-left override.
    #[test]
    fn the_five_bidi_messages_from_production_are_caught() {
        for s in [
            "\u{202E}DRAC NOSIAS\u{FEFF}", // SAISON CARD
            "\u{202E}BCJyM",               // MyJCB
            "\u{2066}\u{202E}【 \u{200B}p\u{FEFF}j.oc.\u{2060}n\u{200C}o\u{200D}zamA 】\u{202C}\u{2069}",
            "\u{202E}n\u{2060}oza\u{2060}m\u{200D}A\u{FEFF} \u{202C}",
            "\u{202E}DRAC NOSIAS",
        ] {
            assert!(
                deception_in(s).bidi_override,
                "a right-to-left override went unnoticed in {s:?}"
            );
        }
    }

    /// **The distinction the module exists for.** These four invisibles
    /// have typographic work to do, and 59 production messages carry
    /// them. Flagging them tells Persian and Hindi senders their mail
    /// looks forged.
    #[test]
    fn the_invisibles_that_typography_needs_are_left_alone() {
        for (label, s) in [
            (
                "emoji family (ZWJ)",
                "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            ),
            ("Persian (ZWNJ)", "می\u{200C}روم"),
            ("Hindi (ZWNJ)", "क\u{200C}ख"),
            ("byte-order mark", "\u{FEFF}Newsletter"),
            ("soft hyphen", "Zusammen\u{00AD}arbeit"),
        ] {
            assert_eq!(
                deception_in(s),
                Deception::default(),
                "{label} was flagged, and it is ordinary typography"
            );
        }
    }

    /// The zero-width padding from the production phish, which is the
    /// weaker of the two signals and is reported separately for that
    /// reason.
    #[test]
    fn zero_width_padding_is_reported_apart_from_bidi() {
        let d = deception_in("M\u{200B}yJC\u{2060}B");
        assert_eq!(
            d,
            Deception {
                bidi_override: false,
                unjustified_zero_width: true
            },
            "padding must not be reported as a bidi override"
        );
        assert!(d.any());
    }

    /// Ordinary mail — including mail in scripts that need shaping, and
    /// real right-to-left text, which needs no override at all.
    #[test]
    fn ordinary_sender_names_are_clean() {
        for s in [
            "MyJCB",
            "Quoraダイジェスト",
            "Amazon.co.jp",
            "GitHub",
            "דואר ישראל", // Hebrew, no override needed
            "البريد",     // Arabic, likewise
            "Ann O'Brien",
            "",
        ] {
            assert_eq!(deception_in(s), Deception::default(), "{s:?}");
        }
    }

    /// Folding several fields: a clean display name beside a tampered
    /// subject still reports.
    #[test]
    fn folding_reports_a_hit_in_any_field() {
        let d = deception_in_any(["Amazon", "【\u{202E}gnihsihp】"]);
        assert!(d.bidi_override);
        assert_eq!(
            deception_in_any(["Amazon", "Your order"]),
            Deception::default()
        );
    }

    /// Every codepoint the module names, asserted one at a time against
    /// the class it belongs to — so a future edit that moves one between
    /// the lists fails here rather than in somebody's inbox.
    #[test]
    fn every_named_codepoint_lands_in_its_own_class() {
        for c in BIDI_CONTROLS {
            let d = deception_in(&c.to_string());
            assert!(
                d.bidi_override,
                "{c:?} is listed as bidi and did not report"
            );
            assert!(
                !d.unjustified_zero_width,
                "{c:?} reported as zero-width too"
            );
        }
        for c in UNJUSTIFIED_ZERO_WIDTH {
            let d = deception_in(&c.to_string());
            assert!(
                d.unjustified_zero_width,
                "{c:?} is listed and did not report"
            );
            assert!(!d.bidi_override, "{c:?} reported as bidi too");
        }
        for c in ['\u{200C}', '\u{200D}', '\u{FEFF}', '\u{00AD}'] {
            assert_eq!(
                deception_in(&c.to_string()),
                Deception::default(),
                "{c:?} has a typographic job and must not be flagged"
            );
        }
    }
}
