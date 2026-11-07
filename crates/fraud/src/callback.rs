//! Phone numbers a message gives its reader to call.
//!
//! A number proves nothing about who answers it, and invoices carry them
//! too, so nothing here judges a message. The host keeps, per reader, the
//! numbers each sender gave before, and the clients' reading pane points
//! out a number the sender has not given before; this module only reads
//! numbers out of text.
//!
//! The reading is the clients' (`web/src/lib/callback-numbers.ts` and its
//! iOS and Android copies), held to the samples in
//! `web/src/lib/callback-numbers.json`: an optional `+` and country code,
//! an optional bracketed area code, then digit groups of at most six
//! joined by one space, dot or hyphen each, with nothing on either side
//! that makes the digits part of something else.

/// Digits after which a number names an order, an invoice or an account.
/// The `labels` of `callback-numbers.json`, which a test holds this to.
pub const LABELS: &[&str] = &[
    "PO",
    "P.O.",
    "Invoice",
    "INV",
    "Order",
    "No.",
    "No",
    "Nr.",
    "#",
    "Ref",
    "Account",
    "Acct",
    "订单",
    "订单号",
    "单号",
    "发票",
    "発注",
    "発注番号",
    "注文",
    "注文番号",
    "請求書",
    "請求書番号",
    "伝票番号",
];

/// Numbers kept from one message.
pub const MAX_NUMBERS: usize = 16;

/// Characters that join two digit groups of one number.
const SEP: [char; 6] = [' ', '\u{a0}', '.', '-', '\u{2010}', '\u{ff0d}'];
/// The same, as `DATE_LIKE` reads them (no no-break space).
const DATE_SEP: [char; 6] = ['-', '.', '/', ' ', '\u{2010}', '\u{ff0d}'];
/// A number starting right after one of these belongs to something else.
const NOT_BEFORE: [char; 13] = [
    '_', '+', '.', ',', '/', '#', '$', '€', '£', '¥', '-', '\u{2010}', '\u{ff0d}',
];
/// …or ending right before one of these.
const NOT_AFTER: [char; 5] = ['_', ':', '-', '\u{2010}', '\u{ff0d}'];
/// Characters before a number that the label check passes over.
const LABEL_TAIL: [char; 6] = [':', '：', '.', '#', '＃', '№'];
const LABEL_WINDOW: usize = 40;
const MIN_DIGITS: usize = 10;
/// A national number without a country code is at most 11 digits in the
/// numbering plans this has to read (JP, US, UK).
const MAX_NATIONAL_DIGITS: usize = 11;
const MAX_DIGITS: usize = 15;

/// The phone numbers in `text`, as written, once each by their digits.
/// Misses some real ones on purpose: a number read where there is none
/// is worse than one passed over.
#[must_use]
pub fn phone_numbers(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let Some(end) = match_at(&chars, i) else {
            i += 1;
            continue;
        };
        let written: String = chars[i..end].iter().collect();
        let d = digits(&written);
        if !seen.contains(&d) && dialable(&written, &d) && !after_label(&chars[..i]) {
            seen.push(d);
            out.push(written);
        }
        i = end;
    }
    out
}

fn is_digit(c: Option<&char>) -> bool {
    c.is_some_and(char::is_ascii_digit)
}

/// The end of the number that starts at `i`, the way a backtracking
/// matcher of the clients' expression finds it: every optional part and
/// every count tried longest first, the first end whose next character
/// lets it stop.
fn match_at(c: &[char], i: usize) -> Option<usize> {
    if i > 0 && (c[i - 1].is_ascii_alphanumeric() || NOT_BEFORE.contains(&c[i - 1])) {
        return None;
    }
    let mut starts = Vec::new();
    if c.get(i) == Some(&'+') {
        for n in (1..=3).rev() {
            if (1..=n).all(|k| is_digit(c.get(i + k))) {
                let p = i + 1 + n;
                if c.get(p).is_some_and(|x| SEP.contains(x)) {
                    starts.push(p + 1);
                }
                starts.push(p);
            }
        }
    }
    starts.push(i);
    for p in starts {
        if let Some(end) = after_country(c, p) {
            return Some(end);
        }
    }
    None
}

/// After the country code: an optional bracketed area code, then the groups.
fn after_country(c: &[char], p: usize) -> Option<usize> {
    if c.get(p) == Some(&'(') {
        for n in (1..=4).rev() {
            if (1..=n).all(|k| is_digit(c.get(p + k))) && c.get(p + 1 + n) == Some(&')') {
                let q = p + 2 + n;
                if c.get(q).is_some_and(|x| SEP.contains(x))
                    && let Some(end) = groups(c, q + 1)
                {
                    return Some(end);
                }
                if let Some(end) = groups(c, q) {
                    return Some(end);
                }
            }
        }
    }
    groups(c, p)
}

/// A group of one to six digits, then one to five more, each after a
/// separator.
fn groups(c: &[char], p: usize) -> Option<usize> {
    for n in (1..=6).rev() {
        if (0..n).all(|k| is_digit(c.get(p + k)))
            && let Some(end) = more_groups(c, p + n, 0)
        {
            return Some(end);
        }
    }
    None
}

fn more_groups(c: &[char], p: usize, had: usize) -> Option<usize> {
    if had < 5 && c.get(p).is_some_and(|x| SEP.contains(x)) {
        for n in (1..=6).rev() {
            if (1..=n).all(|k| is_digit(c.get(p + k)))
                && let Some(end) = more_groups(c, p + 1 + n, had + 1)
            {
                return Some(end);
            }
        }
    }
    (had >= 1 && may_end(c, p)).then_some(p)
}

/// Whether a number may end before `c[p]`.
fn may_end(c: &[char], p: usize) -> bool {
    let Some(next) = c.get(p) else { return true };
    if next.is_ascii_alphanumeric() || NOT_AFTER.contains(next) {
        return false;
    }
    !(matches!(next, '.' | ',' | '/') && is_digit(c.get(p + 1)))
}

fn dialable(written: &str, d: &str) -> bool {
    let max = match written.starts_with('+') {
        true => MAX_DIGITS,
        false => MAX_NATIONAL_DIGITS,
    };
    (MIN_DIGITS..=max).contains(&d.len()) && !date_like(written) && !thousands(written)
}

/// `2026-11-01 12.30`: four digits, then two groups of one or two.
fn date_like(w: &str) -> bool {
    let c: Vec<char> = w.chars().collect();
    let run = |from: usize| {
        c[from.min(c.len())..]
            .iter()
            .take_while(|x| x.is_ascii_digit())
            .count()
    };
    if c.len() < 8 || !c[..4].iter().all(char::is_ascii_digit) || !DATE_SEP.contains(&c[4]) {
        return false;
    }
    let month = run(5);
    if !(1..=2).contains(&month) || !c.get(5 + month).is_some_and(|x| DATE_SEP.contains(x)) {
        return false;
    }
    // `\d{1,2}(?!\d)`: a day of one or two digits, a third not following
    (1..=2).contains(&run(6 + month))
}

/// `1 234 567 890`: an amount grouped by thousands.
fn thousands(w: &str) -> bool {
    let groups: Vec<&str> = w.split([' ', '\u{a0}', '.']).collect();
    let [first, rest @ ..] = groups.as_slice() else {
        return false;
    };
    !rest.is_empty()
        && (1..=3).contains(&first.len())
        && !first.starts_with('0')
        && first.bytes().all(|b| b.is_ascii_digit())
        && rest
            .iter()
            .all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether the text just before a number ends in a label naming an order,
/// an invoice or an account, then nothing but separators.
fn after_label(before: &[char]) -> bool {
    let window = &before[before.len().saturating_sub(LABEL_WINDOW)..];
    let mut end = window.len();
    loop {
        let head = &window[..end];
        if LABELS.iter().any(|l| ends_with_label(head, l)) {
            return true;
        }
        match window[..end].last() {
            Some(x) if x.is_whitespace() || LABEL_TAIL.contains(x) || *x == '-' => end -= 1,
            _ => return false,
        }
    }
}

fn ends_with_label(head: &[char], label: &str) -> bool {
    let l: Vec<char> = label.chars().collect();
    if l.len() > head.len() {
        return false;
    }
    let at = head.len() - l.len();
    let same = head[at..]
        .iter()
        .zip(&l)
        .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()));
    same && (at == 0 || !head[at - 1].is_ascii_alphanumeric())
}

/// `text`'s digits only: what two spellings of one number share.
#[must_use]
pub fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The text of an HTML body as a reader sees it, near enough to read
/// numbers from: tags dropped, block boundaries kept as line breaks so
/// figures in two cells never join, the common entities decoded.
#[must_use]
pub fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..lt]));
        rest = &rest[lt..];
        let Some(gt) = rest.find('>') else {
            rest = "";
            break;
        };
        let closing = rest[1..gt].starts_with('/');
        let tag = rest[1..gt].trim_start_matches('/').to_ascii_lowercase();
        let name: String = tag
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect();
        rest = &rest[gt + 1..];
        if !closing && matches!(name.as_str(), "script" | "style") {
            let close = format!("</{name}");
            rest = match rest.to_ascii_lowercase().find(&close) {
                Some(at) => &rest[at..],
                None => "",
            };
            continue;
        }
        if BLOCKS.contains(&name.as_str()) {
            out.push('\n');
        }
    }
    out.push_str(&decode_entities(rest));
    out
}

/// Elements that end a line of text where they open or close.
const BLOCKS: [&str; 17] = [
    "br", "p", "div", "td", "th", "tr", "li", "ul", "ol", "table", "h1", "h2", "h3", "h4", "h5",
    "h6", "hr",
];

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let end = rest[1..].find(';').map(|i| i + 1).filter(|&i| i <= 10);
        let decoded = end.and_then(|e| entity(&rest[1..e]));
        match (end, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let code = match name {
        "nbsp" => return Some('\u{a0}'),
        "amp" => return Some('&'),
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok(),
        n if n.starts_with('#') => n[1..].parse().ok(),
        _ => None,
    };
    code.and_then(char::from_u32)
}

#[cfg(test)]
mod tests;
