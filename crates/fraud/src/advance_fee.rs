//! A letter that offers its reader a cut of a very large sum.
//!
//! ```text
//! The transaction pertains to an unclaimed "Payable on Death" savings
//! deposit in the sum of Ten Million, Two Hundred Thousand United
//! States Dollars … we shall fill your Company's name as the Foreign
//! Technical Partner to our deceased client … split equally, 50% apiece.
//! ```
//!
//! The advance-fee letter has no stable header. It arrives from forged
//! freemail, from rented shared hosting, and — the one that reached an
//! inbox on 2026-09-28 — from a real company's stolen account with SPF,
//! DKIM and DMARC all passing. What does not change is the offer: a sum
//! in the millions, and the reader's place in it.
//!
//! # Two conditions, both in the text
//!
//! - **a sum of at least a million in a named currency**
//! - **the reader is offered part of it**: `you will retain`, `your
//!   share`, `split equally`, `apiece`, `next of kin`, `as the foreign
//!   partner`, `compensation payment` …
//!
//! Either alone is ordinary. Newsletters report `$57 million` of
//! somebody's revenue every week, and `your share` is how a cloud bill
//! talks. The first version asked for a sum plus a pretext word
//! (`deceased`, `unclaimed`, `kickback`) and convicted a Railway
//! product announcement and a run of newsletters; what separates the
//! fraud is that the money is being promised **to the person reading**.
//!
//! # Measured
//!
//! Over 40,866 production messages on 2026-09-28, with mailing-list
//! mail left out by the caller: 9 caught, all advance-fee fraud from
//! five campaigns, none wrong. One known letter of the kind is missed —
//! it names a sum and a pretext and never says what the reader gets.
//!
//! # Only English
//!
//! The phrases are English because every sample is. A letter in any
//! other language reads as *no offer* and is not convicted, which is
//! the direction to be wrong in.

/// One text part of a message, decoded to a string.
#[derive(Debug, Clone, Copy)]
pub enum Body<'a> {
    /// `text/plain`, read as it is.
    Plain(&'a str),
    /// `text/html`, read with its tags dropped.
    Html(&'a str),
}

/// How much of the text is read. Every sample fits in 8 KiB; the
/// bound is here so a 30 MB body costs the same as a letter.
pub const READ_LIMIT: usize = 256 * 1024;

const OFFERS: &[&str] = &[
    "you will retain",
    "you will be entitled",
    "your share",
    "your percentage",
    "split equally",
    "shared equally",
    "fifty-fifty",
    "fifty fifty",
    " apiece",
    "next of kin",
    "as the beneficiary",
    "as the foreign partner",
    "as the foreign technical partner",
    "compensation payment",
    "partner with me in ",
    "partner with me on ",
    "partner with us in ",
    "partner with us on ",
];

const SCALES: &[&str] = &["million", "billion"];

const CURRENCIES: &[&str] = &["$", "usd", "dollar", "€", "eur", "£", "gbp", "pound"];

/// How far either side of `million` a currency may sit and still be
/// the same sum: `US$25 million`, `$2.1 million Dollars`, `Ten Million,
/// Two Hundred Thousand United States Dollars`.
const SUM_REACH: usize = 60;

/// Whether the text offers its reader a share of a sum in the millions.
#[must_use]
pub fn offers_the_reader_a_sum<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> bool {
    let text = reading(bodies);
    names_a_large_sum(&text) && OFFERS.iter().any(|o| text.contains(o))
}

/// The text a reader sees: tags dropped, the entities these letters
/// use turned back into characters, lowercased, every run of
/// whitespace one space — a phrase broken across a `format=flowed`
/// line is still the phrase.
fn reading<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> String {
    let mut out = String::new();
    for body in bodies {
        if out.len() >= READ_LIMIT {
            break;
        }
        let visible = match body {
            Body::Plain(s) => cut(s, READ_LIMIT - out.len()).to_string(),
            Body::Html(s) => strip_tags(cut(s, READ_LIMIT - out.len())),
        };
        let mut space = true;
        out.push(' ');
        for c in visible.chars() {
            if c.is_whitespace() || c == '\u{a0}' {
                if !space {
                    out.push(' ');
                }
                space = true;
            } else {
                out.extend(c.to_lowercase());
                space = false;
            }
        }
    }
    out
}

fn cut(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => {
                in_tag = false;
                out.push(' ');
            }
            (false, _) => out.push(c),
            (true, _) => {}
        }
    }
    decode_entities(&out)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = cut(tail, 12).find(';') else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "euro" => Some('€'),
            "pound" => Some('£'),
            _ => name.strip_prefix('#').and_then(|n| {
                match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                }
                .and_then(char::from_u32)
            }),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `million` or `billion` with a currency close enough to be its
/// unit, or a figure with two thousands separators next to one:
/// `US$10,200,000.00`.
fn names_a_large_sum(text: &str) -> bool {
    for scale in SCALES {
        for (at, _) in text.match_indices(scale) {
            let before = &text[floor(text, at.saturating_sub(SUM_REACH))..at];
            let after = &text[at + scale.len()..ceil(text, at + scale.len() + SUM_REACH)];
            if has_currency(before) || has_currency(after) {
                return true;
            }
        }
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_digit()) {
            let start = i;
            let mut groups = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let lead = i - start;
            while i + 3 < bytes.len()
                && bytes[i] == b','
                && bytes[i + 1..i + 4].iter().all(u8::is_ascii_digit)
                && bytes.get(i + 4).is_none_or(|b| !b.is_ascii_digit())
            {
                groups += 1;
                i += 4;
            }
            if lead <= 3 && groups >= 2 {
                let before = &text[floor(text, start.saturating_sub(6))..start];
                let after = &text[i..ceil(text, i + 12)];
                if has_currency(before) || has_currency(after) {
                    return true;
                }
            }
        } else {
            i += 1;
        }
    }
    false
}

fn has_currency(s: &str) -> bool {
    CURRENCIES.iter().any(|c| s.contains(c))
}

fn floor(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> bool {
        offers_the_reader_a_sum([Body::Plain(s)])
    }

    /// The one that reached the inbox: plain text, `format=flowed`,
    /// the phrase broken across lines.
    #[test]
    fn the_reported_letter_is_caught() {
        let s = "savings monetary deposit in the sum of  Ten Million, Two \n\
                 Hundred Thousand United States Dollars only (US$10,200,000.00). \
                 ... we shall fill your Company's name as the Foreign Technical \n\
                 Partner to our deceased client ... split equally, 50% apiece.";
        assert!(plain(s));
    }

    /// Its first wave, as HTML with entities between the words.
    #[test]
    fn the_html_waves_are_caught() {
        let s = "<div>in the sum of&nbsp; Ten Million, Two Hundred Thousand United States \
                 Dollars only (US$10,200,000.00).<br> ... This money would be split \
                 equally, 50% apiece.<br></div>";
        assert!(offers_the_reader_a_sum([Body::Html(s)]));
        let w = "<body>kickback money US$25 million ... to partner with me in securing \
                 some funds abroad ... you will retain 20% of the US$25 million</body>";
        assert!(offers_the_reader_a_sum([Body::Html(w)]));
    }

    #[test]
    fn the_other_campaigns_in_the_corpus_are_caught() {
        assert!(plain(
            "Attention COMPENSATION PAYMENT OF $2.1 million Dollars Be informed that"
        ));
        assert!(plain(
            "the sum of $5.2 million ... requiring the identification of a next of kin"
        ));
        assert!(plain(
            "funds of $6.8 Billion from my client the investor, and you will be entitled to 5%"
        ));
    }

    /// Newsletter lines the pretext-word version convicted.
    #[test]
    fn a_reported_sum_with_no_offer_is_not() {
        assert!(!plain(
            "NVIDIA's revenue from data centers grew to $57 million, over 90% of the total"
        ));
        assert!(!plain(
            "Railway raises $100M Series B. Kickback: earn credits when you refer a friend."
        ));
        assert!(!plain(
            "Fill out the survey and you will receive the full results; $423M in funding"
        ));
    }

    #[test]
    fn an_offer_with_no_large_sum_is_not() {
        assert!(!plain("your share of the $40 dinner bill, split equally"));
        assert!(!plain(
            "a million thanks — your share of the credit is well earned"
        ));
    }

    /// Not in the corpus: the same letter in Japanese and Chinese.
    /// Nothing here reads them, and they are delivered.
    #[test]
    fn other_languages_are_not_read() {
        assert!(!plain(
            "1,020万米ドルの未請求の預金について、あなたの取り分は50%です"
        ));
        assert!(!plain("一千万美元的无人认领存款，你将获得百分之五十"));
    }

    #[test]
    fn figures_need_two_separators_and_a_currency() {
        assert!(names_a_large_sum(" us$10,200,000.00 "));
        assert!(names_a_large_sum(" 1,500,000 usd "));
        assert!(!names_a_large_sum(" 10,200,000 page views "));
        assert!(!names_a_large_sum(" $10,200 "));
    }

    #[test]
    fn the_read_is_bounded() {
        let long = "a ".repeat(READ_LIMIT);
        let text = format!("{long} $5 million, your share");
        assert!(!plain(&text));
    }

    #[test]
    fn entities_decode() {
        assert_eq!(
            decode_entities("a&nbsp;b &#8220;c&#8221; &amp; &bogus;"),
            "a b \u{201c}c\u{201d} & &bogus;"
        );
    }

    /// Found on the production corpus: a bare `&` with multibyte text
    /// inside the entity window.
    #[test]
    fn a_bare_ampersand_before_multibyte_text_is_kept() {
        assert_eq!(decode_entities("R&D’s ‘plan’"), "R&D’s ‘plan’");
        assert_eq!(decode_entities("&日本語のテキスト"), "&日本語のテキスト");
    }
}
