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
    // A run of rule characters, still being counted. Dropped once it
    // reaches `RULE_RUN`, and written out as ordinary text if it stops
    // short — `--` is how people write a dash.
    let mut run_len = 0usize;
    // The characters of a run still being counted, so one short enough
    // to be text can be written back verbatim. Only `RULE_RUN - 1` can
    // ever be needed: at `RULE_RUN` the run is a bar and is dropped.
    let mut run = ['\0'; RULE_RUN];
    // A `text/plain` part is returned as the sender wrote it, and some
    // senders write HTML into one. Over 10,000 production
    // conversations 16 arrive that way, and 198 of the 202 entities in
    // them are `&nbsp;` or `&zwnj;` — invisible characters spelled out
    // as text, which is the one kind a preview must not show. They are
    // turned back into characters here and then handled like any
    // other, so `&nbsp;` collapses and `&zwnj;` is dropped.
    //
    // Only the invisible ones. `&amp;` and `&ldquo;` are content, and
    // a preview that decoded them would be a half-built HTML parser
    // living in the wrong crate — four occurrences, and the fix for
    // those is upstream of here.
    let text = decode_invisible_entities(text);
    for ch in text.chars() {
        if is_rule_char(ch) {
            // **Any** rule characters in a row are one run, not one run
            // per character. `───────┬────` is a single bar with a tee
            // in it; counting each stretch separately left the tee
            // behind as a one-character run and wrote it back as text.
            // The kinds are kept so a short run can be written out
            // exactly as it arrived.
            if run_len < run.len() {
                run[run_len] = ch;
            }
            run_len += 1;
            continue;
        }
        flush_run(&mut out, &mut kept, &mut pending_space, &run, run_len, max);
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
    flush_run(&mut out, &mut kept, &mut pending_space, &run, run_len, max);
    out
}

/// Replace `&nbsp;` and `&zwnj;` with the characters they name.
///
/// A borrow when there is nothing to do, which is 9,984 conversations
/// in 10,000.
fn decode_invisible_entities(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("&nbsp;") && !text.contains("&zwnj;") {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.replace("&nbsp;", "\u{00A0}")
            .replace("&zwnj;", "\u{200C}"),
    )
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
    run: &[char],
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
    for ch in run.iter().take(len) {
        if *kept >= max {
            out.push('…');
            return;
        }
        out.push(*ch);
        *kept += 1;
    }
}

/// Characters mail uses to draw a line across the page.
///
/// A hyphen inside a word or a date is a single character and never
/// reaches `RULE_RUN`; three in a row are a rule wherever they appear.
///
/// **Box drawing is most of it.** Measured over 10,000 production
/// conversations, 975 — nearly one row in ten — open with a bar the
/// ASCII list here does not know: `─` alone occurs 46,147 times,
/// `━` 10,986, and Japanese newsletters build headers out of `┬ ┴ │
/// ┏ ┗ ╋` around a title:
///
/// ```text
/// ───────┬──── [Ameba]│[PR] ───────┴──── # [Amebaおすすめキャンペーン]…
/// ━━━━━━━━━━━ じゃらんnetメールマガジン ━━━━━━━━ 2026年08月31日 本メールは…
/// ```
///
/// The whole U+2500 block is here rather than the characters seen so
/// far: every one of them draws a line, and a newsletter that
/// switches to `╍` next month should not need another measurement.
/// The fullwidth forms come from the same family of senders.
fn is_rule_char(ch: char) -> bool {
    matches!(
        ch,
        '-' | '=' | '_' | '*' | '~' | '\u{2014}' | '\u{2013}' | '\u{00B7}' | '\u{2022}'
            | '\u{2015}'                 // horizontal bar
            | '\u{2500}'..='\u{257F}'    // box drawing, the whole block
            | '\u{FF0D}'                 // fullwidth hyphen-minus
            | '\u{FF1D}'                 // fullwidth equals
            | '\u{FF5E}'                 // fullwidth tilde
            | '\u{25A0}'..='\u{25AF}'    // squares, used the same way
    )
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

    /// **The bars a Japanese newsletter actually draws**, verbatim
    /// from production. 975 of 10,000 conversations opened with one of
    /// these — `\u{2500}` alone occurs 46,147 times — and the ASCII
    /// list did not know any of them.
    #[test]
    fn box_drawing_is_a_rule_line_too() {
        assert_eq!(
            preview_line(
                "\u{2501}\u{2501}\u{2501}\u{2501}\u{2501} \u{3058}\u{3083}\u{3089}\u{3093}net \u{2501}\u{2501}\u{2501}\u{2501} 2026\u{5e74}08\u{6708}31\u{65e5}",
                120
            ),
            "\u{3058}\u{3083}\u{3089}\u{3093}net 2026\u{5e74}08\u{6708}31\u{65e5}"
        );
        // A tee in the middle of a bar is part of the same bar. Counted
        // per character kind, the tee was a run of one and came back as
        // text — which is how `\u{252c}` ended up leading a preview.
        assert_eq!(
            preview_line(
                "\u{2500}\u{2500}\u{2500}\u{252c}\u{2500}\u{2500}\u{2500} [Ameba] \u{2500}\u{2500}\u{2500}\u{2534}\u{2500}\u{2500}\u{2500} PR",
                120
            ),
            "[Ameba] PR"
        );
        // And the fullwidth family, which the same senders use.
        assert_eq!(
            preview_line("A\n\u{ff1d}\u{ff1d}\u{ff1d}\u{ff1d}\nB", 120),
            "A B"
        );
        assert_eq!(
            preview_line("A\n\u{2015}\u{2015}\u{2015}\u{2015}\nB", 120),
            "A B"
        );
    }

    /// A run of *different* rule characters is still one run. The
    /// version that counted each kind separately wrote every change of
    /// character back as text, so a mixed bar came through in pieces.
    #[test]
    fn a_mixed_run_is_one_run() {
        assert_eq!(preview_line("A -=- B", 120), "A B");
        assert_eq!(preview_line("A -= B", 120), "A -= B", "two is still text");
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

    /// Verbatim from production: a sender writing HTML entities into
    /// a `text/plain` part. 16 conversations in 10,000, and both of
    /// these name characters that must not be visible.
    #[test]
    fn an_invisible_entity_spelled_out_is_still_invisible() {
        assert_eq!(
            preview_line(
                "\u{cca8}\u{bd80}\u{d30c}\u{c77c}&nbsp; \u{b2e4}\u{c6b4}\u{b85c}\u{b4dc}",
                120
            ),
            "\u{cca8}\u{bd80}\u{d30c}\u{c77c} \u{b2e4}\u{c6b4}\u{b85c}\u{b4dc}"
        );
        assert_eq!(preview_line("Sale&zwnj;&zwnj;ends", 120), "Saleends");
    }

    /// And what stays. Decoding these would be a half-built HTML
    /// parser in a crate about collapsing whitespace, and `&amp;` in a
    /// preview is ugly rather than broken.
    #[test]
    fn a_content_entity_is_left_where_it_is() {
        assert_eq!(preview_line("Tom &amp; Jerry", 120), "Tom &amp; Jerry");
        assert_eq!(
            preview_line("&ldquo;quoted&rdquo;", 120),
            "&ldquo;quoted&rdquo;"
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
