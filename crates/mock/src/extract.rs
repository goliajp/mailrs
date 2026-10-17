//! One-time codes and links found in a message's text.
//!
//! A code is a candidate token near a keyword that is not an amount, a
//! date, a phone number or an order number. Codes come back nearest to
//! a keyword first, ties in order of appearance.

mod links;

pub use links::links;

// a candidate this far (in characters) from every keyword is ignored
const KEYWORD_WINDOW: usize = 120;
// an order keyword this far before a candidate marks it an order number
const ORDER_WINDOW: usize = 30;
// a digit sequence with this many digits around a candidate is a phone number
const PHONE_DIGITS: usize = 9;

// ascii keywords match whole words, case-insensitively, with a plural s
const ASCII_KEYWORDS: &[&str] = &[
    "code",
    "verification",
    "verify",
    "otp",
    "one-time",
    "passcode",
    "pin",
];
const CJK_KEYWORDS: &[&str] = &[
    "認証",
    "確認コード",
    "ワンタイム",
    "パスコード",
    "認証コード",
    "验证码",
    "校验码",
    "动态码",
    "驗證碼",
];
const ASCII_ORDER: &[&str] = &["order"];
const CJK_ORDER: &[&str] = &["注文", "订单", "訂單"];

const CURRENCY_BEFORE: &[char] = &['$', '¥', '€', '£', '￥'];
const CURRENCY_AFTER: &[char] = &['円', '元'];

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

impl Span {
    fn gap(self, other: Span) -> usize {
        // at most one side is positive; overlapping spans give 0
        other.start.saturating_sub(self.end) + self.start.saturating_sub(other.end)
    }

    fn overlaps(self, other: Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

struct Candidate {
    span: Span,
    value: String,
}

pub fn codes(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().map(fold_char).collect();
    let lower: Vec<char> = chars.iter().map(char::to_ascii_lowercase).collect();
    let keywords = find_keywords(&lower, ASCII_KEYWORDS, CJK_KEYWORDS);
    if keywords.is_empty() {
        return Vec::new();
    }
    let orders = find_keywords(&lower, ASCII_ORDER, CJK_ORDER);
    let urls = url_spans(&lower);

    let mut ranked: Vec<(usize, Candidate)> = candidates(&chars)
        .into_iter()
        .filter(|c| !urls.iter().any(|u| u.overlaps(c.span)))
        .filter(|c| !is_amount(&chars, c.span) && !is_date(&chars, c))
        .filter(|c| !is_phone(&chars, c.span) && !is_order(&orders, c.span))
        .filter_map(|c| {
            let distance = keywords.iter().map(|k| k.gap(c.span)).min()?;
            (distance <= KEYWORD_WINDOW).then_some((distance, c))
        })
        .collect();
    ranked.sort_by_key(|(distance, c)| (*distance, c.span.start));

    let mut out: Vec<String> = Vec::new();
    for (_, c) in ranked {
        if !out.contains(&c.value) {
            out.push(c.value);
        }
    }
    out
}

/// Full-width Latin letters and digits folded to their ASCII forms.
fn fold_char(c: char) -> char {
    match c {
        'Ａ'..='Ｚ' | 'ａ'..='ｚ' | '０'..='９' => {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        }
        _ => c,
    }
}

fn find_keywords(lower: &[char], ascii: &[&str], cjk: &[&str]) -> Vec<Span> {
    let mut out = Vec::new();
    for kw in ascii {
        let kw: Vec<char> = kw.chars().collect();
        for span in occurrences(lower, &kw) {
            if ascii_word_boundary(lower, span) {
                out.push(span);
            }
        }
    }
    for kw in cjk {
        let kw: Vec<char> = kw.chars().collect();
        out.extend(occurrences(lower, &kw));
    }
    out
}

fn occurrences(hay: &[char], needle: &[char]) -> impl Iterator<Item = Span> {
    hay.windows(needle.len())
        .enumerate()
        .filter(move |(_, w)| *w == needle)
        .map(move |(i, _)| Span {
            start: i,
            end: i + needle.len(),
        })
}

// "code" in "encode" or "pin" in "shipping" is not a keyword
fn ascii_word_boundary(lower: &[char], span: Span) -> bool {
    let before_ok = span.start == 0 || !lower[span.start - 1].is_ascii_alphanumeric();
    let letter_at = |i: usize| lower.get(i).is_some_and(char::is_ascii_alphabetic);
    let after_ok = !letter_at(span.end) || (lower[span.end] == 's' && !letter_at(span.end + 1));
    before_ok && after_ok
}

// a code inside a link is a query parameter, not something to type
fn url_spans(lower: &[char]) -> Vec<Span> {
    let mut out = Vec::new();
    for scheme in ["http://", "https://"] {
        let scheme: Vec<char> = scheme.chars().collect();
        for span in occurrences(lower, &scheme) {
            let end = lower[span.end..]
                .iter()
                .position(|c| c.is_whitespace())
                .map_or(lower.len(), |i| span.end + i);
            out.push(Span {
                start: span.start,
                end,
            });
        }
    }
    out
}

fn alnum_runs(chars: &[char]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_alphanumeric() {
            i += 1;
        }
        out.push(Span { start, end: i });
    }
    out
}

fn candidates(chars: &[char]) -> Vec<Candidate> {
    let all_digits = |s: Span| chars[s.start..s.end].iter().all(char::is_ascii_digit);
    let text = |s: Span| chars[s.start..s.end].iter().collect::<String>();
    let runs = alnum_runs(chars);
    let mut out = Vec::new();
    let mut i = 0;
    while i < runs.len() {
        let run = runs[i];
        let len = run.end - run.start;
        let digits = all_digits(run);
        if digits
            && len == 3
            && let Some(&next) = runs.get(i + 1)
            && next.start == run.end + 1
            && matches!(chars[run.end], ' ' | '-')
            && next.end - next.start == 3
            && all_digits(next)
        {
            out.push(Candidate {
                span: Span {
                    start: run.start,
                    end: next.end,
                },
                value: text(run) + &text(next),
            });
            i += 2;
            continue;
        }
        let token = &chars[run.start..run.end];
        let alphanumeric = (6..=8).contains(&len)
            && token
                .iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && token.iter().any(char::is_ascii_digit)
            && token.iter().any(char::is_ascii_uppercase);
        if (digits && (4..=8).contains(&len)) || alphanumeric {
            out.push(Candidate {
                span: run,
                value: text(run),
            });
        }
        i += 1;
    }
    out
}

fn prev_non_space(chars: &[char], at: usize) -> Option<char> {
    chars[..at].iter().rev().find(|c| **c != ' ').copied()
}

fn next_non_space(chars: &[char], at: usize) -> Option<char> {
    chars[at..].iter().find(|c| **c != ' ').copied()
}

fn digit_at(chars: &[char], i: Option<usize>) -> bool {
    i.and_then(|i| chars.get(i))
        .is_some_and(char::is_ascii_digit)
}

// a currency sign before or 円 / 元 after, or the run is one group of a
// longer number such as 1234.00 or 12,345
fn is_amount(chars: &[char], span: Span) -> bool {
    if prev_non_space(chars, span.start).is_some_and(|c| CURRENCY_BEFORE.contains(&c))
        || next_non_space(chars, span.end).is_some_and(|c| CURRENCY_AFTER.contains(&c))
    {
        return true;
    }
    let before = span.start.checked_sub(1).map(|i| chars[i]);
    let after = chars.get(span.end).copied();
    (matches!(before, Some('.' | ',')) && digit_at(chars, span.start.checked_sub(2)))
        || (matches!(after, Some('.' | ',')) && digit_at(chars, Some(span.end + 1)))
}

// a year next to - / . or 年, or any number joined to another by a slash
fn is_date(chars: &[char], c: &Candidate) -> bool {
    let before = c.span.start.checked_sub(1).map(|i| chars[i]);
    let after = chars.get(c.span.end).copied();
    let year = c.value.len() == 4 && (c.value.starts_with("19") || c.value.starts_with("20"));
    if year
        && (matches!(before, Some('-' | '/' | '年'))
            || matches!(after, Some('-' | '/' | '.' | '年')))
    {
        return true;
    }
    if matches!(after, Some('年' | '月' | '日')) {
        return true;
    }
    (before == Some('/') && digit_at(chars, c.span.start.checked_sub(2)))
        || (after == Some('/') && digit_at(chars, Some(c.span.end + 1)))
}

// the run sits in a sequence of digits, + - ( ) and spaces holding at
// least nine digits, like +81 3-1234-5678
fn is_phone(chars: &[char], span: Span) -> bool {
    let phone_char = |c: &char| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')');
    let start = chars[..span.start]
        .iter()
        .rposition(|c| !phone_char(c))
        .map_or(0, |i| i + 1);
    let end = chars[span.end..]
        .iter()
        .position(|c| !phone_char(c))
        .map_or(chars.len(), |i| span.end + i);
    chars[start..end]
        .iter()
        .filter(|c| c.is_ascii_digit())
        .count()
        >= PHONE_DIGITS
}

fn is_order(orders: &[Span], span: Span) -> bool {
    orders
        .iter()
        .any(|o| o.end <= span.start && span.start - o.end <= ORDER_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_code() {
        assert_eq!(codes("Your verification code is 482913"), ["482913"]);
        assert_eq!(
            codes(
                "[GitHub] Please verify your device\nVerification code: 482913\nIf you did not ask, ignore this."
            ),
            ["482913"]
        );
    }

    #[test]
    fn japanese_full_width_code() {
        assert_eq!(codes("認証コード：１２３４５６"), ["123456"]);
        assert_eq!(
            codes("ワンタイムパスワード 7391 を入力してください"),
            ["7391"]
        );
    }

    #[test]
    fn chinese_code_with_validity_period() {
        assert_eq!(codes("您的验证码是 739201，5分钟内有效"), ["739201"]);
        assert_eq!(codes("您的驗證碼為 739201"), ["739201"]);
    }

    #[test]
    fn grouped_digits_are_joined() {
        assert_eq!(codes("Your code: 123 456"), ["123456"]);
        assert_eq!(codes("Your code: 123-456"), ["123456"]);
    }

    #[test]
    fn uppercase_alphanumeric_code() {
        assert_eq!(codes("Your one-time passcode: AB12CD"), ["AB12CD"]);
        assert!(codes("Your code: ABCDEF").is_empty());
        assert!(codes("Your code: ab12cd").is_empty());
    }

    #[test]
    fn nearest_to_a_keyword_comes_first() {
        let text = "Account 55554444 was used to sign in.\n\n\n\
                    Enter this code: 482913";
        assert_eq!(codes(text), ["482913", "55554444"]);
    }

    #[test]
    fn far_from_any_keyword_is_ignored() {
        let text = format!("verify\n{}\n482913", "x".repeat(130));
        assert!(codes(&text).is_empty());
        assert!(codes("Meeting room 4821 at noon").is_empty());
    }

    #[test]
    fn keyword_inside_another_word_does_not_count() {
        assert!(codes("Shipping 482913 encoded").is_empty());
        assert_eq!(codes("Your PIN 4829"), ["4829"]);
        assert_eq!(codes("backup codes: 48291733"), ["48291733"]);
    }

    #[test]
    fn billing_amounts_are_not_codes() {
        assert!(codes("Your invoice code is ready, total $1234.00").is_empty());
        assert!(codes("確認コード 合計 3,980円").is_empty());
        assert!(codes("確認コード 合計 3980円").is_empty());
        assert!(codes("验证码 金额 ￥5000").is_empty());
        assert!(codes("verification fee 12,345.67").is_empty());
    }

    #[test]
    fn dates_are_not_codes() {
        assert!(codes("Please verify by 2026-10-17").is_empty());
        assert!(codes("Please verify by 2026/10/17").is_empty());
        assert!(codes("認証の期限は2026年10月17日です").is_empty());
        assert!(codes("verify before 10/17/2026").is_empty());
    }

    #[test]
    fn phone_numbers_are_not_codes() {
        assert!(codes("Verification team, tel +81 3-1234-5678").is_empty());
        assert!(codes("Verification team (03) 1234 5678").is_empty());
        assert!(codes("verify: call 123-456-789").is_empty());
    }

    #[test]
    fn order_numbers_are_not_codes() {
        assert!(codes("order 48291733 verification").is_empty());
        assert!(codes("ご注文番号 48291733 の確認コード").is_empty());
        assert!(codes("订单 48291733 验证码").is_empty());
    }

    #[test]
    fn codes_inside_links_are_skipped() {
        assert!(codes("verify: https://x.test/verify?c=482913").is_empty());
    }

    #[test]
    fn duplicates_are_dropped() {
        assert_eq!(codes("Your code is 482913\nCode: 482913"), ["482913"]);
    }

    #[test]
    fn empty_and_garbage() {
        assert!(codes("").is_empty());
        assert!(codes("code").is_empty());
        assert!(codes("code 12").is_empty());
    }
}
