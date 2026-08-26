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
//! - **Rule lines.** Plain-text mail draws them with dashes, and once
//!   the newlines are collapsed away they arrive in the middle of the
//!   preview as a long bar: nearly every row on a phone opened
//!   `Hello HAO, ------------------------------ …`. They separate
//!   paragraphs that are no longer on separate lines, so on one line
//!   they say nothing at all.

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
    // A run of the same rule character, still being counted. Dropped
    // once it reaches `RULE_RUN`, and written out as ordinary text if
    // it stops short — `--` is how people write a dash.
    let mut run_char = '\0';
    let mut run_len = 0usize;
    for ch in text.chars() {
        if is_rule_char(ch) {
            if ch == run_char {
                run_len += 1;
            } else {
                flush_run(
                    &mut out,
                    &mut kept,
                    &mut pending_space,
                    run_char,
                    run_len,
                    max,
                );
                run_char = ch;
                run_len = 1;
            }
            continue;
        }
        flush_run(
            &mut out,
            &mut kept,
            &mut pending_space,
            run_char,
            run_len,
            max,
        );
        run_char = '\0';
        run_len = 0;
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
    flush_run(
        &mut out,
        &mut kept,
        &mut pending_space,
        run_char,
        run_len,
        max,
    );
    out
}

/// How many of the same rule character make a line rather than a dash.
const RULE_RUN: usize = 3;

/// Write back a run that turned out to be too short to be a rule.
///
/// It takes `pending_space` because the space before it has not been
/// written yet: the collapse defers one, and a run that jumps the queue
/// turns `wait -- what?` into `wait-- what?`. Found by the test that
/// says short runs are text.
fn flush_run(
    out: &mut String,
    kept: &mut usize,
    pending_space: &mut bool,
    ch: char,
    len: usize,
    max: usize,
) {
    if len == 0 || len >= RULE_RUN {
        return;
    }
    if *pending_space {
        out.push(' ');
        *kept += 1;
        *pending_space = false;
    }
    for _ in 0..len {
        if *kept >= max {
            out.push('…');
            return;
        }
        out.push(ch);
        *kept += 1;
    }
}

/// Characters mail uses to draw a line across the page.
///
/// Deliberately short. A hyphen inside a word or a date is a single
/// character and never reaches `RULE_RUN`; three in a row are a rule
/// wherever they appear.
fn is_rule_char(ch: char) -> bool {
    matches!(ch, '-' | '=' | '_' | '*' | '~' | '—' | '–' | '·' | '•')
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

    /// The bar that opened nearly every row on a phone.
    ///
    /// Plain-text mail draws a rule with dashes on its own line. Once
    /// the newlines around it are collapsed, it lands mid-sentence as a
    /// long bar that means nothing on one line.
    #[test]
    fn a_rule_line_is_not_the_preview() {
        assert_eq!(
            preview_line(
                "Hello HAO,\n------------------------------\nYour receipt",
                120
            ),
            "Hello HAO, Your receipt"
        );
        assert_eq!(preview_line("A\n====\nB", 120), "A B");
        assert_eq!(preview_line("A\n____________\nB", 120), "A B");
        assert_eq!(preview_line("A\n***\nB", 120), "A B");
        assert_eq!(preview_line("A\n———\nB", 120), "A B");
    }

    /// What the backfill leans on.
    ///
    /// Rows stored before this knew about rule lines hold the bar as
    /// literal dashes on one line already. Running the same function
    /// over that stored string has to clear it — otherwise the sweep
    /// would have to re-read every message from disk to repair a line.
    #[test]
    fn a_second_pass_over_a_stored_preview_clears_the_bar() {
        let stored = preview_line(
            "Hello HAO,\n------------------------------\nYour receipt",
            120,
        );
        let stale = "Hello HAO, ------------------------------ Your receipt";
        assert_eq!(preview_line(stale, 120), stored);
        // And running it again changes nothing.
        assert_eq!(preview_line(&stored, 120), stored);
    }

    /// And what must survive it. Two dashes are how people write a
    /// dash, a hyphen lives inside words and dates, and a rule that is
    /// only two characters long is not a rule.
    #[test]
    fn short_runs_are_text_and_stay() {
        assert_eq!(preview_line("wait -- what?", 120), "wait -- what?");
        assert_eq!(
            preview_line("e-mail on 2026-08-26", 120),
            "e-mail on 2026-08-26"
        );
        assert_eq!(preview_line("a--b", 120), "a--b");
        assert_eq!(preview_line("5 * 3 = 15", 120), "5 * 3 = 15");
    }

    /// A run that reaches the limit does not lose the cut mark.
    #[test]
    fn a_short_run_at_the_limit_is_still_marked() {
        assert_eq!(preview_line("abc--", 4), "abc-…");
    }

    #[test]
    fn a_soft_hyphen_does_not_survive() {
        assert_eq!(
            preview_line("Rechnungs\u{00AD}nummer", 120),
            "Rechnungsnummer"
        );
    }
}
