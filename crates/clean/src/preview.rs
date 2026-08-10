//! The one line under the subject in a conversation list.
//!
//! Small, and worth its own home because two paths produce it — the
//! outbound send and the inbound drain — and a list where your own
//! messages read differently from everyone else's looks broken in a way
//! nobody can name.
//!
//! Two things the obvious implementation gets wrong on real mail:
//!
//! - **Zero-width padding.** 240 of 5,600 real HTML messages carry a run
//!   of zero-width characters, one of them 552 of them: senders pad the
//!   preheader so the client's preview stops before the next paragraph
//!   leaks into it. Kept, they make a preview that is present and
//!   invisible — worse than an empty one, because nothing looks wrong.
//! - **Non-breaking spaces**, in 781 of the same 5,600. A collapse that
//!   only knows about ASCII space leaves a line of them.

/// The first `max` characters of `text` as a single line.
///
/// Every run of whitespace — including the Unicode ones — becomes one
/// space, zero-width characters are dropped rather than spaced, and an
/// ellipsis marks a cut. An empty result means the body had nothing
/// readable in it, which is a real answer and not a failure.
pub fn preview_line(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max * 4));
    let mut pending_space = false;
    // Counted, not re-measured: `out.chars().count()` on every kept
    // character makes the cost of a preview quadratic in its own length.
    let mut kept = 0usize;
    for ch in text.chars() {
        if is_zero_width(ch) {
            // Dropped, not turned into a space: a run of 552 of them is
            // padding between two words that belong next to each other.
            continue;
        }
        if ch.is_whitespace() {
            // Only if something has already been written — this also
            // trims the front.
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            kept += 1;
            pending_space = false;
        }
        if kept >= max {
            out.push('…');
            return out;
        }
        out.push(ch);
        kept += 1;
    }
    out
}

/// Characters that occupy no width and carry no meaning in a preview.
///
/// **Not** the zero-width joiner. It occupies no width either, and
/// dropping it looked consistent — but joining is its whole job:
/// `👨‍👩‍👧` is three people and two joiners, and without them a preview
/// shows three separate emoji. One real subject in the corpus is built
/// that way.
///
/// The soft hyphen is here for the same reason as the rest: it is a
/// hint about where a word *may* break, and a preview that keeps it
/// shows a hyphen in the middle of a word on the one line where it will
/// never be broken.
fn is_zero_width(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}' // soft hyphen
            | '\u{200B}' // zero-width space
            | '\u{200C}' // zero-width non-joiner

            | '\u{2060}' // word joiner
            | '\u{FEFF}' // zero-width no-break space / BOM
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_a_wrapped_body_into_one_line() {
        let body = "Please review\r\nthe figures\tbefore Friday.\n\nThe numbers moved.";
        assert_eq!(
            preview_line(body, 120),
            "Please review the figures before Friday. The numbers moved."
        );
    }

    #[test]
    fn trims_both_ends() {
        assert_eq!(preview_line("\n\n  hello  \n\n", 120), "hello");
    }

    /// 240 of 5,600 real HTML messages. Spacing them out instead of
    /// dropping them gives a preview of blanks that looks like a bug in
    /// the list rather than a trick in the mail.
    #[test]
    fn drops_preheader_padding() {
        let padded = format!("Sale{}ends today", "\u{200C}\u{00A0}".repeat(60));
        assert_eq!(preview_line(&padded, 120), "Sale ends today");
    }

    #[test]
    fn a_body_of_only_padding_is_empty() {
        assert_eq!(
            preview_line(&"\u{200B}\u{FEFF}\u{00A0}".repeat(50), 120),
            ""
        );
        assert_eq!(preview_line("", 120), "");
        assert_eq!(preview_line("   \n\t ", 120), "");
    }

    /// 781 of 5,600 use `&nbsp;`, which `char::is_whitespace` knows about
    /// and a check for `' '` does not.
    #[test]
    fn a_non_breaking_space_is_a_space() {
        assert_eq!(preview_line("a\u{00A0}\u{00A0}b", 120), "a b");
    }

    #[test]
    fn marks_a_cut_and_counts_characters_not_bytes() {
        assert_eq!(preview_line("abcdefghij", 4), "abcd…");
        // Japanese is three bytes a character; a byte-counting cap would
        // stop after two of these and could split one in half.
        assert_eq!(preview_line("請求書のご送付につきまして", 5), "請求書のご…");
    }

    #[test]
    fn a_body_exactly_at_the_limit_is_not_marked() {
        assert_eq!(preview_line("abcd", 4), "abcd");
    }

    /// A family is one glyph made of three people and two joiners.
    /// Dropping the joiners as "zero width" turns it into three.
    #[test]
    fn a_joined_emoji_stays_joined() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(
            preview_line(&format!("Sale {family} today"), 120),
            format!("Sale {family} today")
        );
    }

    #[test]
    fn a_soft_hyphen_does_not_survive() {
        assert_eq!(
            preview_line("Rechnungs\u{00AD}nummer", 120),
            "Rechnungsnummer"
        );
    }
}
