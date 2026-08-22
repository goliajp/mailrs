#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};

/// A date, and the time beside it if the writer gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The date, with its year resolved against the reference.
    pub date: NaiveDate,
    /// The time, when one was written near the date. `None` means the
    /// writer named a day and not an hour — an all-day proposal, not a
    /// meeting at midnight.
    pub time: Option<NaiveTime>,
    /// Byte range of the matched date in the input, so a client can
    /// underline exactly what it read.
    pub span: (usize, usize),
    /// The text as written, for a card that quotes it back rather than
    /// reformatting it and hoping the reader recognises it.
    pub text: String,
}

impl Candidate {
    /// The instant, when a time was given.
    pub fn naive(&self) -> Option<NaiveDateTime> {
        self.time.map(|t| self.date.and_time(t))
    }
}

/// Every date this finds in `text`, in the order they appear.
///
/// `reference` resolves bare years and is normally the message's own
/// `Date:` header — not "now", which would make the same message read
/// differently tomorrow.
///
/// Returns at most `LIMIT` candidates: a newsletter full of dates is
/// not a meeting proposal, and offering twenty events is offering
/// none.
pub fn find(text: &str, reference: NaiveDate) -> Vec<Candidate> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && out.len() < LIMIT {
        // Byte index, character text: a match can only start where a
        // character does. Stepping a byte at a time through Japanese
        // otherwise slices one in half.
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        // Only start a match at a boundary, so "August" inside a word
        // is not a date and neither is the `21` in `x21`.
        if i > 0 && !is_boundary(bytes[i - 1]) {
            i += 1;
            continue;
        }
        if let Some((end, date)) = match_date(text, i, reference) {
            let matched = text[i..end].to_string();
            let time = time_near(text, end);
            out.push(Candidate {
                date,
                time,
                span: (i, end),
                text: matched,
            });
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

/// The cap on how many dates one message can propose.
pub const LIMIT: usize = 8;

/// The most distinct dates a message can offer and still be read as a
/// proposal. Above this it is a listing, and a listing offers nothing.
pub const MOST_A_PROPOSAL_NAMES: usize = 3;

/// The dates in `text` that read as somebody proposing a time.
///
/// [`find`] answers "is this a date". This answers the different and
/// harder question the reader actually has — "is somebody suggesting we
/// meet then" — and the two must not be conflated. A support reply
/// quoting eight SMTP rejection timestamps contains eight dates and
/// proposes nothing; offering all eight is worse than offering none,
/// because the reader now has to judge each one.
///
/// Four things disqualify a date, each of them a shape rather than a
/// guess about meaning:
///
/// - **It is in quoted text.** A line beginning `>`, or anything below
///   an `On … wrote:` / `-----Original Message-----` boundary, was
///   written by somebody else in another context. This alone removed
///   every chip in the 2026-08-21 report.
/// - **It is a machine timestamp** — an ISO instant carrying a clock,
///   sub-seconds or a zone. `2026-08-25T04:00:00.123Z` is a log line.
/// - **It has already happened.** A proposal is about the future;
///   `reference` is the message's own date, so this reads the same way
///   tomorrow.
/// - **There are too many.** Past [`MOST_A_PROPOSAL_NAMES`] distinct
///   days, the message is enumerating, not asking.
///
/// Repeats of the same day and hour collapse to one, so writing
/// "the 25th … all of the 25th" offers one event rather than two.
pub fn propose(text: &str, reference: NaiveDate) -> Vec<Candidate> {
    let own = writers_own_text(text);
    let mut out: Vec<Candidate> = Vec::new();
    for c in find(own, reference) {
        // Today, with no hour beside it, is a statement about now
        // rather than a question about later: a proposal for today has
        // to name a time or there is nothing to agree to. A bank's
        // 「8月22日に…がありました」 was offered as an event on the day
        // it arrived, 2026-08-22.
        let states_today = c.date == reference && c.time.is_none();
        if c.date < reference || states_today || is_machine_timestamp(own, &c) {
            continue;
        }
        if out.iter().any(|k| k.date == c.date && k.time == c.time) {
            continue;
        }
        out.push(c);
    }
    if out.len() > MOST_A_PROPOSAL_NAMES {
        return Vec::new();
    }
    out
}

/// Everything above the first quote or reply boundary.
///
/// Deliberately crude: the boundary forms below cover what mail
/// clients actually emit, and a missed boundary costs a stray offer
/// while an over-eager one silently loses a real proposal. When in
/// doubt this keeps text.
fn writers_own_text(text: &str) -> &str {
    let mut end = text.len();
    for (i, line) in line_offsets(text) {
        let t = line.trim_start();
        let quoted = t.starts_with('>');
        let boundary = t.starts_with("-----Original Message")
            || t.starts_with("________________________________")
            || (t.starts_with("On ") && t.trim_end().ends_with("wrote:"))
            || (t.starts_with("At ") && t.trim_end().ends_with("wrote:"));
        if quoted || boundary {
            end = i;
            break;
        }
    }
    &text[..end]
}

/// `(byte offset, line)` for each line, newline excluded.
fn line_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0usize;
    text.split_inclusive('\n').map(move |l| {
        let start = at;
        at += l.len();
        (start, l.trim_end_matches(['\n', '\r']))
    })
}

/// Whether the match sits inside an ISO 8601 instant.
///
/// The tell is what follows the date: `T` and a clock. A person
/// proposing a time writes "on the 25th at 4pm", not `T04:00:00.123Z`.
fn is_machine_timestamp(text: &str, c: &Candidate) -> bool {
    let after = &text[c.span.1..];
    let b = after.as_bytes();
    b.len() >= 3 && (b[0] == b'T' || b[0] == b't') && b[1].is_ascii_digit() && b[2].is_ascii_digit()
}

fn is_boundary(b: u8) -> bool {
    !b.is_ascii_alphanumeric()
}

/// A date starting exactly at `at`, and where it ends.
fn match_date(text: &str, at: usize, reference: NaiveDate) -> Option<(usize, NaiveDate)> {
    let rest = &text[at..];
    if let Some(r) = match_iso(rest) {
        return Some((at + r.0, r.1));
    }
    if let Some(r) = match_cjk(rest, reference) {
        return Some((at + r.0, r.1));
    }
    if let Some(r) = match_month_name(rest, reference) {
        return Some((at + r.0, r.1));
    }
    None
}

/// `2026-08-21`, and only that: `2026/08/21` is the same shape but
/// `08/09/2026` is not, and the two cannot be told apart by looking.
fn match_iso(rest: &str) -> Option<(usize, NaiveDate)> {
    let b = rest.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let digits = |r: std::ops::Range<usize>| -> Option<u32> {
        std::str::from_utf8(&b[r]).ok()?.parse().ok()
    };
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = digits(0..4)? as i32;
    let m = digits(5..7)?;
    let d = digits(8..10)?;
    // A longer run of digits is a serial number, not a date.
    if b.len() > 10 && b[10].is_ascii_digit() {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d).map(|date| (10, date))
}

/// `8月21日`, with an optional `2026年` before it. Written the same way
/// in Japanese and Chinese, which is why this is one arm and not two.
fn match_cjk(rest: &str, reference: NaiveDate) -> Option<(usize, NaiveDate)> {
    let mut idx = 0usize;
    let mut year: Option<i32> = None;
    if let Some((len, n)) = leading_number(&rest[idx..])
        && rest[idx + len..].starts_with('年')
    {
        year = Some(n as i32);
        idx += len + '年'.len_utf8();
    }
    let (mlen, month) = leading_number(&rest[idx..])?;
    if !rest[idx + mlen..].starts_with('月') {
        return None;
    }
    idx += mlen + '月'.len_utf8();
    let (dlen, day) = leading_number(&rest[idx..])?;
    if !rest[idx + dlen..].starts_with('日') {
        return None;
    }
    idx += dlen + '日'.len_utf8();
    let date = build(year, month, day, reference)?;
    Some((idx, date))
}

/// `August 21`, `Aug 21st`, `21 August`, each with an optional year.
fn match_month_name(rest: &str, reference: NaiveDate) -> Option<(usize, NaiveDate)> {
    // Day-first: `21 August 2026`.
    if let Some((dlen, day)) = leading_number(rest) {
        let after = &rest[dlen..];
        let sp = after.len() - after.trim_start().len();
        if sp > 0
            && let Some((mlen, month)) = leading_month(&after[sp..])
        {
            let mut idx = dlen + sp + mlen;
            let year = trailing_year(&rest[idx..])
                .inspect(|(ylen, _)| idx += ylen)
                .map(|(_, y)| y);
            return build(year, month, day, reference).map(|d| (idx, d));
        }
    }
    // Month-first: `August 21`, `Aug 21st, 2026`.
    let (mlen, month) = leading_month(rest)?;
    let after = &rest[mlen..];
    let sp = after.len() - after.trim_start().len();
    if sp == 0 {
        return None;
    }
    let (dlen, day) = leading_number(&after[sp..])?;
    let mut idx = mlen + sp + dlen;
    // `21st` / `21nd` / `21rd` / `21th`.
    for suffix in ["st", "nd", "rd", "th"] {
        if rest[idx..].to_ascii_lowercase().starts_with(suffix) {
            idx += 2;
            break;
        }
    }
    let year = trailing_year(&rest[idx..])
        .inspect(|(ylen, _)| idx += ylen)
        .map(|(_, y)| y);
    build(year, month, day, reference).map(|d| (idx, d))
}

/// `, 2026` or ` 2026` after a date.
fn trailing_year(rest: &str) -> Option<(usize, i32)> {
    let mut idx = 0usize;
    if rest.starts_with(',') {
        idx += 1;
    }
    let after = &rest[idx..];
    let sp = after.len() - after.trim_start().len();
    if sp == 0 && idx == 0 {
        return None;
    }
    idx += sp;
    let (ylen, n) = leading_number(&rest[idx..])?;
    if ylen != 4 {
        return None;
    }
    Some((idx + ylen, n as i32))
}

fn leading_number(s: &str) -> Option<(usize, u32)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 || end > 4 {
        return None;
    }
    s[..end].parse().ok().map(|n| (end, n))
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn leading_month(s: &str) -> Option<(usize, u32)> {
    let lower = s.to_ascii_lowercase();
    for (i, full) in MONTHS.iter().enumerate() {
        if lower.starts_with(full) {
            return Some((full.len(), i as u32 + 1));
        }
    }
    for (i, full) in MONTHS.iter().enumerate() {
        let abbr = &full[..3];
        if lower.starts_with(abbr) {
            // `Sept` as well as `Sep`, and not `Marching`.
            let len = if lower[3..].starts_with('t') && *full == "september" {
                4
            } else {
                3
            };
            let next = lower[len..].chars().next();
            if next.is_none_or(|c| !c.is_ascii_alphabetic()) {
                return Some((len, i as u32 + 1));
            }
        }
    }
    None
}

/// Build the date, resolving a missing year against the reference.
///
/// **Rolled forward when the result is more than a month behind the
/// reference.** Mail proposing a meeting proposes a future one, so
/// "August 21" written in December means the next one — the rule Apple
/// Mail and Gmail both apply, and the one that stops a December mail
/// offering an event eight months in the past.
fn build(year: Option<i32>, month: u32, day: u32, reference: NaiveDate) -> Option<NaiveDate> {
    if let Some(y) = year {
        return NaiveDate::from_ymd_opt(y, month, day);
    }
    let same = NaiveDate::from_ymd_opt(reference.year(), month, day)?;
    if (reference - same).num_days() > 31 {
        return NaiveDate::from_ymd_opt(reference.year() + 1, month, day);
    }
    Some(same)
}

/// A time written just after a date — `at 2pm`, ` 14:00`, `14時30分`.
///
/// Bounded to a short window so that a date on one line and a time
/// three paragraphs later are not read as one appointment.
fn time_near(text: &str, from: usize) -> Option<NaiveTime> {
    const WINDOW: usize = 24;
    // Back off to a character boundary: the window is a byte count and
    // the text is routinely Japanese, where a naive slice lands inside
    // a character and panics.
    let mut end = text.len().min(from + WINDOW);
    while end > from && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut window = &text[from..end];
    // Trim the connectives a person writes between the two.
    for lead in [" at ", " from ", "、", " ", "(", "（", "午後", "午前"] {
        if let Some(stripped) = window.strip_prefix(lead) {
            window = stripped;
        }
    }
    let pm_word = text[from..end].contains("午後");
    let am_word = text[from..end].contains("午前");
    for start in 0..window.len().min(WINDOW) {
        if !window.is_char_boundary(start) {
            continue;
        }
        if start > 0 && !is_boundary(window.as_bytes()[start - 1]) {
            continue;
        }
        if let Some(t) = match_time(&window[start..], pm_word, am_word) {
            return Some(t);
        }
    }
    None
}

fn match_time(s: &str, pm_word: bool, am_word: bool) -> Option<NaiveTime> {
    let (hlen, hour) = leading_number(s)?;
    if hour > 23 {
        return None;
    }
    let rest = &s[hlen..];
    let (minute, mut idx) = if let Some(after) = rest.strip_prefix(':') {
        let (mlen, m) = leading_number(after)?;
        if mlen != 2 || m > 59 {
            return None;
        }
        (m, hlen + 1 + mlen)
    } else if let Some(after) = rest.strip_prefix('時') {
        let m = leading_number(after)
            .filter(|(l, _)| after[*l..].starts_with('分'))
            .map(|(_, m)| m)
            .unwrap_or(0);
        (m, hlen + '時'.len_utf8())
    } else {
        (0, hlen)
    };
    let tail = s[idx..].trim_start().to_ascii_lowercase();
    let mut hour = hour;
    if tail.starts_with("pm") || pm_word {
        if hour < 12 {
            hour += 12;
        }
        idx += 2;
    } else if tail.starts_with("am") || am_word {
        if hour == 12 {
            hour = 0;
        }
        idx += 2;
    } else if minute == 0 && !s[..idx].contains(':') && !s[..idx].contains('時') {
        // A bare number is a number. "see you 3" is not three o'clock,
        // and reading it as one is how a client offers to schedule a
        // page reference.
        return None;
    }
    let _ = idx;
    NaiveTime::from_hms_opt(hour, minute, 0)
}
