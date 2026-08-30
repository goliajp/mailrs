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

/// Zero-width characters that **do** have a typographic job — in
/// some context.
///
/// A zero-width joiner is how an emoji family is built and how many
/// Indic scripts form conjuncts; a byte-order mark is a legitimate
/// artefact of transcoding. None of them may be listed as
/// unjustified outright.
///
/// Wedged between two alphanumerics they have no job at all. That is
/// what [`Deception::zero_width_inside_a_word`] reads, and it is
/// what separates 17 phishing subjects from Duolingo's 22.
const ZERO_WIDTH_ANYWHERE: &[char] = &[
    '\u{200B}', // ZERO WIDTH SPACE
    '\u{200C}', // ZERO WIDTH NON-JOINER      — legitimate in Indic scripts
    '\u{200D}', // ZERO WIDTH JOINER          — legitimate in emoji sequences
    '\u{2060}', // WORD JOINER
    '\u{FEFF}', // ZERO WIDTH NO-BREAK SPACE  — legitimate as a BOM
    '\u{180E}', // MONGOLIAN VOWEL SEPARATOR
];

/// Whether a zero-width character between two of these has no
/// possible typographic job.
///
/// An **allow-list**, and it has to be. The first version asked
/// `char::is_alphanumeric` on both sides, which is true of Arabic
/// and Devanagari letters — so `می\u{200c}روم` and `क\u{200c}ख`
/// would have been reported as forged. A zero-width non-joiner is
/// how those scripts are written.
///
/// It was caught by `the_invisibles_that_typography_needs_are_left_alone`,
/// which existed already and says in its own comment that flagging
/// them "tells Persian and Hindi senders their mail looks forged".
/// The corpus the wider version measured 17-for-17 on contains no
/// Persian or Hindi mail at all — **a corpus not containing a case
/// is not evidence the case does not arise.**
///
/// So: Latin letters, digits, Han, Kana and Hangul, where no
/// zero-width character has ever had work to do. An unfamiliar
/// script is not flagged, which is the safe direction.
fn no_zero_width_belongs_between(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c,
            '\u{3040}'..='\u{30FF}'   // Hiragana + Katakana
            | '\u{31F0}'..='\u{31FF}' // Katakana phonetic extensions
            | '\u{4E00}'..='\u{9FFF}' // CJK unified ideographs
            | '\u{3400}'..='\u{4DBF}' // CJK extension A
            | '\u{F900}'..='\u{FAFF}' // CJK compatibility ideographs
            | '\u{AC00}'..='\u{D7AF}' // Hangul syllables
            | '\u{FF10}'..='\u{FF19}' // fullwidth digits
            | '\u{FF21}'..='\u{FF3A}' // fullwidth Latin capitals
            | '\u{FF41}'..='\u{FF5A}' // fullwidth Latin small
        )
}

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
    /// A zero-width character with an alphanumeric on **both** sides.
    ///
    /// ```text
    /// 【S\u{200b}A\u{200c}I\u{200d}S\u{feff}O\u{200b}N】本人認証サービス…
    /// 【J\u{200d}CB】本人確\u{200c}認（利用者認\u{2060}証）のお願い
    /// ```
    ///
    /// SAISON with four invisible characters through it, JCB with
    /// three. It defeats a filter that looks for the brand name while
    /// rendering identically to the reader — which is the entire
    /// point, and is a thing no sender does to their own name.
    ///
    /// **Conclusive, where [`Self::unjustified_zero_width`] is not.**
    /// That one is one production message in forty and holds nothing
    /// on its own. This one is 17 of 35,575, and all seventeen are
    /// phishing: SAISON, JCB four times, 楽天カード, SMBC, ANA twice,
    /// Amazon three times, 3D セキュア twice.
    ///
    /// The difference is the neighbours. Duolingo puts a zero-width
    /// character in 22 subjects and none of them are between two
    /// letters — they are emoji joiners and left-to-right marks
    /// around a user's name, both of which do real work.
    pub zero_width_inside_a_word: bool,
}

impl Deception {
    /// Whether anything at all was found.
    pub fn any(self) -> bool {
        self.bidi_override || self.unjustified_zero_width || self.zero_width_inside_a_word
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
    // Previous and next character, so a zero-width one can be judged
    // by its neighbours rather than by itself. A single pass: the
    // window is two characters wide and nothing is re-read.
    let mut prev: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if BIDI_CONTROLS.contains(&c) {
            out.bidi_override = true;
        } else if UNJUSTIFIED_ZERO_WIDTH.contains(&c) {
            out.unjustified_zero_width = true;
        }
        if ZERO_WIDTH_ANYWHERE.contains(&c)
            && prev.is_some_and(no_zero_width_belongs_between)
            && chars
                .peek()
                .copied()
                .is_some_and(no_zero_width_belongs_between)
        {
            out.zero_width_inside_a_word = true;
        }
        prev = Some(c);
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
        out.zero_width_inside_a_word |= d.zero_width_inside_a_word;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seventeen production subjects, and what they render as.
    #[test]
    fn a_brand_name_split_by_invisible_characters_is_found() {
        for (text, renders) in [
            (
                "\u{200b}S\u{200b}A\u{200c}I\u{200d}S\u{feff}O\u{200b}N",
                "SAISON",
            ),
            ("J\u{200d}CB", "JCB"),
            ("J\u{2060}C\u{200b}B", "JCB"),
            ("Am\u{200c}azon.co.jp", "Amazon.co.jp"),
            ("A\u{200d}NA", "ANA"),
            ("e\u{200b}+\u{feff}p\u{200c}l\u{200d}u\u{200b}s", "e+plus"),
            // The zero-widths run through the Japanese too, so the
            // neighbours here are a Latin letter and a katakana.
            ("3D\u{200b}\u{30bb}\u{2060}\u{30ad}", "3D セキ"),
            ("\u{672c}\u{200c}\u{4eba}\u{8a8d}\u{8a3c}", "本人認証"),
        ] {
            assert!(
                deception_in(text).zero_width_inside_a_word,
                "missed {renders}"
            );
        }
    }

    /// Duolingo's 22, which are the reason this is not simply "any
    /// zero-width character". An emoji joiner and a left-to-right
    /// mark both do real work.
    #[test]
    fn a_joiner_doing_its_job_is_not_deception() {
        for text in [
            "\u{1f469}\u{200d}\u{1f467}", // a family emoji
            "\u{1f92f} 认真的？居然是那个\u{200e}Duo_167a7972\u{200e}吗？",
            "\u{1f46f} \u{200e}Muxin\u{200e}想和你交个朋友！",
            "plain text with no tricks",
            "",
            // The scripts a zero-width character belongs in. These
            // are the reason the neighbour test is an allow-list.
            "\u{645}\u{6cc}\u{200c}\u{631}\u{648}\u{645}", // Persian, ZWNJ
            "\u{915}\u{200c}\u{916}",                      // Hindi, ZWNJ
        ] {
            assert!(
                !deception_in(text).zero_width_inside_a_word,
                "wrongly caught {text:?}"
            );
        }
    }

    /// At an edge it has only one neighbour, so it cannot be inside
    /// a word — and a lone leading joiner is what a truncated
    /// transcode leaves behind.
    #[test]
    fn a_zero_width_character_at_an_edge_is_not_inside_anything() {
        assert!(!deception_in("\u{feff}Subject").zero_width_inside_a_word);
        assert!(!deception_in("Subject\u{200b}").zero_width_inside_a_word);
        assert!(!deception_in("\u{200d}").zero_width_inside_a_word);
    }

    /// The two readings are separate: the old one does not see a
    /// joiner at all, and the new one does not fire on a zero-width
    /// space between two spaces.
    #[test]
    fn the_two_readings_do_not_stand_in_for_each_other() {
        let joined = deception_in("J\u{200d}CB");
        assert!(joined.zero_width_inside_a_word);
        assert!(
            !joined.unjustified_zero_width,
            "U+200D is legitimate elsewhere"
        );

        let spaced = deception_in("a \u{200b} b");
        assert!(spaced.unjustified_zero_width);
        assert!(!spaced.zero_width_inside_a_word);
    }

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
                unjustified_zero_width: true,
                // `M\u{200b}yJC\u{2060}B` is also the sharper shape —
                // both characters have a letter on either side. That
                // is not an accident of this fixture: it was written
                // from a phishing subject, and the narrower reading
                // was measured on that whole family afterwards.
                zero_width_inside_a_word: true,
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
