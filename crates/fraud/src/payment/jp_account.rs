//! Japanese domestic bank accounts written out as a transfer destination.
//!
//! A domestic account number has no check digit, so a number alone proves
//! nothing. What a message can prove is the whole structure of a transfer
//! destination: a bank name, a branch (`渋谷支店`, `本店営業部`, `〇一八店`,
//! `店番123`), an account type (`普通` / `当座` / `貯蓄`, `普通預金`, `(普)`,
//! `フツウ`, `ﾌﾂｳ`) and a seven-digit 口座番号, full-width digits included.
//!
//! **Window.** The parts are looked for around each standalone seven-digit
//! number: up to [`LINES_BEFORE`] lines before its line and [`LINES_AFTER`]
//! lines after, and at most [`CHARS_BEFORE`] characters before it and
//! [`CHARS_AFTER`] after, whichever is tighter. That holds one written
//! line (`三菱UFJ銀行 渋谷支店 普通 1234567`) and an invoice's 振込先 block
//! with one label and one value per line (銀行名 / 支店名 / 口座種別 /
//! 口座番号, a table flattened to text the same way), and keeps a bank
//! named in one paragraph from pairing with a number three paragraphs on.
//! Of each part, the one nearest the number is taken.
//!
//! **ゆうちょ銀行 記号・番号** is recognised too: `記号` and a five-digit
//! code starting with 1, then `番号` and up to eight digits within 40
//! characters (or `記号番号 10120-12345671`), with ゆうちょ, 郵便局 or
//! 郵便貯金 named inside the same window.
//!
//! **Two levels.** An account whose bank is on the list of licensed banks
//! ([`super::banks`]) is verified; a structure whose bank is not (a typo, a
//! misread scan, a name the list lacks) is still recorded, as suspected.
//!
//! **Masked.** The number leaves here only as its last three digits in
//! [`JpAccount::masked`] (`三菱UFJ銀行 渋谷支店 普通 ****567`) and inside the
//! hash in [`JpAccount::key`].
//!
//! Not recognised: a bank named only by a short form without `銀行` and
//! without a `銀行名` label (`みずほ 渋谷支店 …`), and kana-only bank names
//! (`ミツビシユーエフジエイ`) as a transfer's kana line writes them.

use super::account_key;
use super::banks::{BankList, fold_width, normalize};

/// Accounts one text yields, at most.
pub const MAX_ACCOUNTS: usize = 4;
/// Lines before the number's line the other parts may be on.
pub const LINES_BEFORE: usize = 8;
/// Lines after the number's line the other parts may be on.
pub const LINES_AFTER: usize = 3;
/// Characters before the number the other parts may start at.
pub const CHARS_BEFORE: usize = 240;
/// Characters after the number the other parts may end at.
pub const CHARS_AFTER: usize = 80;
/// The longest bank or branch name read, in characters.
const NAME_MAX: usize = 16;

/// One account found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpAccount {
    /// The bank is on the list of licensed banks.
    pub verified: bool,
    /// Bank, branch, account type and the number's last three digits.
    pub masked: String,
    /// The hash of the account's identity, for the per-sender history.
    pub key: String,
}

const BANK_SUFFIXES: [&str; 9] = [
    "銀行",
    "信用金庫",
    "信用組合",
    "労働金庫",
    "農業協同組合",
    "信金",
    "信組",
    "労金",
    "農協",
];

/// Words a letter writes in front of the bank's name.
const LEAD_INS: [&str; 12] = [
    "お振込み先",
    "お振込先",
    "お振込",
    "振り込み先",
    "振込先",
    "振込",
    "入金先",
    "送金先",
    "ご指定",
    "指定",
    "下記",
    "取引",
];

const TYPES: [(&str, &str); 14] = [
    ("普通預金", "普通"),
    ("当座預金", "当座"),
    ("貯蓄預金", "貯蓄"),
    ("普通", "普通"),
    ("当座", "当座"),
    ("貯蓄", "貯蓄"),
    ("フツウ", "普通"),
    ("トウザ", "当座"),
    ("チョチク", "貯蓄"),
    ("ﾌﾂｳ", "普通"),
    ("ﾄｳｻﾞ", "当座"),
    ("ﾁｮﾁｸ", "貯蓄"),
    ("普", "普通"),
    ("当", "当座"),
];

const SEPARATORS: &str = " :|]】)）/";

/// The accounts written out in `text`, banks checked against `banks`.
#[must_use]
pub fn find(text: &str, banks: &BankList) -> Vec<JpAccount> {
    let chars: Vec<char> = text
        .chars()
        .filter(|c| *c != '\r')
        .map(fold_width)
        .map(|c| match c {
            '\t' => ' ',
            _ => c,
        })
        .collect();
    let mut out: Vec<JpAccount> = Vec::new();
    let found = digit_runs(&chars)
        .into_iter()
        .filter(|&(at, len)| len == 7 && standalone(&chars, at, len))
        .filter_map(|(at, _)| domestic(&chars, at, banks))
        .chain(yucho(&chars, banks));
    for a in found {
        if out.len() == MAX_ACCOUNTS {
            break;
        }
        if !out.iter().any(|b| b.key == a.key) {
            out.push(a);
        }
    }
    out
}

fn domestic(chars: &[char], at: usize, banks: &BankList) -> Option<JpAccount> {
    let (lo, hi) = window(chars, at, at + 7);
    let kind = nearest(account_types(chars, lo, hi), at)?;
    let branch = nearest(branches(chars, lo, hi), at)?;
    let bank = nearest(bank_names(chars, lo, hi), at)?;
    let number: String = chars[at..at + 7].iter().collect();
    // one key per bank and number: a listed bank under its current
    // normalised name (so a former name keys like the current one), any
    // other under its own normalised name, which is what the listed name
    // would be, so a bank added to the list keeps its accounts' keys
    let canonical = banks.canonical(&bank);
    let name = canonical.clone().unwrap_or_else(|| normalize(&bank));
    Some(JpAccount {
        verified: canonical.is_some(),
        key: account_key(&format!("jp:{name}:{number}")),
        masked: format!("{bank} {branch} {kind} ****{}", &number[4..]),
    })
}

fn yucho(chars: &[char], banks: &BankList) -> Vec<JpAccount> {
    let mut out = Vec::new();
    for p in occurrences(chars, 0, chars.len(), "記号") {
        let mut i = p + 2;
        if starts(chars, i, "番号") {
            i += 2;
        }
        let i = skip_separators(chars, i, chars.len());
        let Some(kigou) = digits_at(chars, i, 5..=5).filter(|k| k.starts_with('1')) else {
            continue;
        };
        let after = i + 5;
        let compact = match chars.get(after) {
            Some('-') | Some(' ') => digits_at(chars, after + 1, 1..=8),
            _ => None,
        };
        let bangou = compact.or_else(|| {
            let end = (after + 40).min(chars.len());
            let q = occurrences(chars, after, end, "番号").next()?;
            digits_at(chars, skip_separators(chars, q + 2, end), 1..=8)
        });
        let Some(bangou) = bangou else { continue };
        let (lo, hi) = window(chars, p, after);
        let named = ["ゆうちょ", "郵便局", "郵便貯金"]
            .iter()
            .any(|n| occurrences(chars, lo, hi, n).next().is_some());
        if !named {
            continue;
        }
        let shown = &bangou[bangou.len().saturating_sub(3)..];
        let identity = format!("yucho:{kigou}:{}", bangou.trim_start_matches('0'));
        out.push(JpAccount {
            verified: banks.canonical("ゆうちょ").is_some(),
            masked: format!("ゆうちょ銀行 記号{kigou} ****{shown}"),
            key: account_key(&identity),
        });
    }
    out
}

/// The span the parts of an account at `start..end` may be found in.
fn window(chars: &[char], start: usize, end: usize) -> (usize, usize) {
    let mut lo = start;
    let mut lines = 0;
    while lo > 0 && start - lo < CHARS_BEFORE {
        if chars[lo - 1] == '\n' {
            if lines == LINES_BEFORE {
                break;
            }
            lines += 1;
        }
        lo -= 1;
    }
    let mut hi = end;
    let mut lines = 0;
    while hi < chars.len() && hi - end < CHARS_AFTER {
        if chars[hi] == '\n' {
            if lines == LINES_AFTER {
                break;
            }
            lines += 1;
        }
        hi += 1;
    }
    (lo, hi)
}

fn account_types(chars: &[char], lo: usize, hi: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        let hit = TYPES.iter().find(|(written, _)| {
            let len = written.chars().count();
            starts(chars, i, written)
                && i + len <= hi
                && (len > 1
                    || (!name_char_at(chars, i.wrapping_sub(1)) && !name_char_at(chars, i + 1)))
        });
        match hit {
            Some((written, kind)) => {
                out.push((i, kind.to_string()));
                i += written.chars().count();
            }
            None => i += 1,
        }
    }
    out
}

fn branches(chars: &[char], lo: usize, hi: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for suffix in ["支店", "出張所", "営業部"] {
        for p in occurrences(chars, lo, hi, suffix) {
            let end = p + suffix.chars().count();
            if label_follows(chars, end) {
                continue;
            }
            let s = run_back(chars, lo, p, true);
            if s < p {
                out.push((s, chars[s..end].iter().collect()));
            }
        }
    }
    for p in occurrences(chars, lo, hi, "店") {
        let numerals = (lo..p)
            .rev()
            .take_while(|&k| "〇一二三四五六七八九".contains(chars[k]))
            .count();
        let main = p > lo && chars[p - 1] == '本';
        if numerals >= 3 || (main && !starts(chars, p + 1, "営業部")) {
            let s = p - numerals.max(usize::from(main));
            out.push((s, chars[s..=p].iter().collect()));
        }
    }
    for p in occurrences(chars, lo, hi, "支店名") {
        let i = skip_separators(chars, p + 3, hi);
        let e = run_forward(chars, i, hi);
        if e > i {
            out.push((i, chars[i..e].iter().collect()));
        }
    }
    for label in ["支店番号", "支店コード", "店舗番号", "店番号", "店番"] {
        for p in occurrences(chars, lo, hi, label) {
            let i = skip_separators(chars, p + label.chars().count(), hi);
            if let Some(code) = digits_at(chars, i, 3..=3) {
                out.push((p, format!("店番{code}")));
            }
        }
    }
    out
}

fn bank_names(chars: &[char], lo: usize, hi: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for suffix in BANK_SUFFIXES {
        for p in occurrences(chars, lo, hi, suffix) {
            let end = p + suffix.chars().count();
            if label_follows(chars, end) {
                continue;
            }
            let s = run_back(chars, lo, p, false);
            let name = strip_lead_ins(chars[s..end].iter().collect());
            if name.chars().count() > suffix.chars().count() {
                out.push((s, name));
            }
        }
    }
    for label in ["金融機関名", "銀行名"] {
        for p in occurrences(chars, lo, hi, label) {
            let i = skip_separators(chars, p + label.chars().count(), hi);
            let e = run_forward(chars, i, hi);
            if e >= i + 2 {
                out.push((i, chars[i..e].iter().collect()));
            }
        }
    }
    for p in occurrences(chars, lo, hi, "ゆうちょ") {
        if !starts(chars, p + 4, "銀行") {
            out.push((p, "ゆうちょ".to_string()));
        }
    }
    out
}

/// `名`, `コード` or `番号` after a word makes the word a label.
fn label_follows(chars: &[char], end: usize) -> bool {
    starts(chars, end, "名") || starts(chars, end, "コード") || starts(chars, end, "番号")
}

fn strip_lead_ins(mut name: String) -> String {
    while let Some(rest) = LEAD_INS.iter().find_map(|l| name.strip_prefix(l)) {
        name = rest.to_string();
    }
    name.replace("株式会社", "")
}

/// Where the name ending at `end` starts: back over name characters, no
/// further than `lo` or [`NAME_MAX`], and, for a branch, not into the bank
/// name written right before it.
fn run_back(chars: &[char], lo: usize, end: usize, stop_at_bank: bool) -> usize {
    let mut s = end;
    while s > lo && end - s < NAME_MAX && is_name_char(chars[s - 1]) {
        if stop_at_bank && BANK_SUFFIXES.iter().any(|b| ends_at(chars, s, b)) {
            break;
        }
        s -= 1;
    }
    s
}

/// Where the name starting at `start` ends: over name characters, and
/// through the first bank suffix, no further.
fn run_forward(chars: &[char], start: usize, hi: usize) -> usize {
    let mut e = start;
    while e < hi && e - start < NAME_MAX && is_name_char(chars[e]) {
        e += 1;
        if BANK_SUFFIXES.iter().any(|b| ends_at(chars, e, b)) {
            break;
        }
    }
    e
}

fn is_name_char(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}' | '\u{3005}' | '\u{FF66}'..='\u{FF9F}'
        | 'A'..='Z' | 'a'..='z' | '&')
}

fn name_char_at(chars: &[char], i: usize) -> bool {
    chars.get(i).is_some_and(|&c| is_name_char(c))
}

fn nearest(found: Vec<(usize, String)>, at: usize) -> Option<String> {
    found
        .into_iter()
        .min_by_key(|(p, _)| p.abs_diff(at))
        .map(|(_, s)| s)
}

fn skip_separators(chars: &[char], mut i: usize, hi: usize) -> usize {
    while i < hi && SEPARATORS.contains(chars[i]) {
        i += 1;
    }
    i
}

/// The digits starting at `i`, when their count is in `len` and no digit
/// follows.
fn digits_at(chars: &[char], i: usize, len: std::ops::RangeInclusive<usize>) -> Option<String> {
    let n = chars
        .get(i..)?
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    len.contains(&n).then(|| chars[i..i + n].iter().collect())
}

/// Every run of ASCII digits, as `(start, len)`.
fn digit_runs(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let n = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if n > 0 {
            out.push((i, n));
        }
        i += n.max(1);
    }
    out
}

/// A run not part of a phone number, a postcode, a decimal or a grouped
/// amount.
fn standalone(chars: &[char], at: usize, len: usize) -> bool {
    let before = at.checked_sub(1).map(|i| chars[i]);
    let after = chars.get(at + len).copied();
    !matches!(before, Some('-' | '〒' | '.' | ',' | '+')) && !matches!(after, Some('-' | '.' | ','))
}

fn starts(chars: &[char], i: usize, pat: &str) -> bool {
    let mut rest = chars.get(i..).unwrap_or(&[]).iter();
    pat.chars().all(|p| rest.next() == Some(&p))
}

fn ends_at(chars: &[char], end: usize, pat: &str) -> bool {
    let n = pat.chars().count();
    end >= n && starts(chars, end - n, pat)
}

/// Where `pat` starts, wholly inside `lo..hi`.
fn occurrences<'a>(
    chars: &'a [char],
    lo: usize,
    hi: usize,
    pat: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    let n = pat.chars().count();
    (lo..hi.saturating_sub(n.saturating_sub(1))).filter(move |&i| starts(chars, i, pat))
}

#[cfg(test)]
#[path = "jp_account_tests.rs"]
mod tests;
