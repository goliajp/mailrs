//! The two times a message carries: the one its sender wrote in
//! `Date:`, and the one this deployment stamped on arrival in the
//! topmost `Received:`.
//!
//! The first is a claim, the second is ours. A sweep over stored mail
//! has to compare against the second rather than the clock: a message
//! dated a month ahead stops being "in the future" once that month
//! has passed, and the sweep would release what the live path held.

const HEAD_LIMIT: usize = 16 * 1024;

/// Parse an RFC 5322 date into epoch seconds, tolerating what real
/// mailers send: a trailing `(comment)` and a day-of-week that
/// disagrees with the date.
pub fn parse_date(s: &str) -> Option<i64> {
    parse_fixed(s).map(|dt| dt.timestamp())
}

/// The zone a `Date:` was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Seconds east of UTC.
    Offset(i32),
    /// `-0000`: the time is UTC and the sender's local zone is not known
    /// (RFC 5322 section 3.3).
    Unknown,
}

/// The zone of the `Date:` header, or `None` when it does not parse.
pub fn date_header_zone(raw: &[u8]) -> Option<Zone> {
    let v = first_header(raw, "date")?;
    let dt = parse_fixed(&v)?;
    let unknown = without_comment(&v)
        .split_whitespace()
        .last()
        .is_some_and(|z| z == "-0000");
    Some(match unknown {
        true => Zone::Unknown,
        false => Zone::Offset(dt.offset().local_minus_utc()),
    })
}

fn without_comment(s: &str) -> &str {
    s.split(" (").next().unwrap_or(s).trim_end()
}

fn parse_fixed(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return Some(dt);
    }
    let no_comment = without_comment(s);
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(no_comment) {
        return Some(dt);
    }
    let no_dow = match no_comment.find(", ") {
        Some(idx) => no_comment[idx + 2..].trim_start(),
        None => no_comment,
    };
    chrono::DateTime::parse_from_rfc2822(no_dow).ok()
}

/// The `Date:` header, in epoch seconds.
pub fn date_header_epoch(raw: &[u8]) -> Option<i64> {
    first_header(raw, "date").and_then(|v| parse_date(&v))
}

/// When this deployment received the message: the date after the last
/// `;` of the topmost `Received:`, which the receiver prepends.
pub fn received_epoch(raw: &[u8]) -> Option<i64> {
    let v = first_header(raw, "received")?;
    parse_date(v.rsplit_once(';')?.1)
}

/// How many seconds `Date:` claims to be ahead of `arrival`; zero when
/// it is not ahead or when there is no date to read.
pub fn date_ahead_of(raw: &[u8], arrival: Option<i64>) -> i64 {
    match (date_header_epoch(raw), arrival) {
        (Some(date), Some(arrival)) => (date - arrival).max(0),
        _ => 0,
    }
}

// the first occurrence of `name`, unfolded
fn first_header(raw: &[u8], name: &str) -> Option<String> {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut collected: Option<String> = None;
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some(value) = collected.as_mut() {
            if line.starts_with([' ', '\t']) {
                value.push(' ');
                value.push_str(line.trim());
                continue;
            }
            break;
        }
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case(name)
        {
            collected = Some(value.trim().to_string());
        }
    }
    collected
}

#[cfg(test)]
mod tests {
    use super::*;

    // the message that prompted the rule, headers as production stored them
    const DATED_A_MONTH_AHEAD: &[u8] = b"Received: from mail.golia.ai (207.57.121.154:45424)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
\tfor <lihao@golia.jp>; Mon, 12 Oct 2026 18:40:33 +0000\r\n\
From: <base2026@tintgate.com>\r\n\
Date: Thu, 12 Nov 2026 18:40:32 +0000\r\n\
\r\n\
body\r\n";

    #[test]
    fn reads_both_times_off_the_message() {
        assert_eq!(received_epoch(DATED_A_MONTH_AHEAD), Some(1_791_830_433));
        assert_eq!(date_header_epoch(DATED_A_MONTH_AHEAD), Some(1_794_508_832));
        let ahead = date_ahead_of(DATED_A_MONTH_AHEAD, received_epoch(DATED_A_MONTH_AHEAD));
        assert_eq!(ahead, 31 * 86_400 - 1);
    }

    #[test]
    fn a_date_in_the_past_is_not_ahead() {
        let raw = b"Received: x; Mon, 12 Oct 2026 18:40:33 +0000\r\nDate: Mon, 12 Oct 2026 18:00:00 +0000\r\n\r\n";
        assert_eq!(date_ahead_of(raw, received_epoch(raw)), 0);
    }

    #[test]
    fn nothing_to_compare_is_not_ahead() {
        let no_date = b"Received: x; Mon, 12 Oct 2026 18:40:33 +0000\r\n\r\n";
        assert_eq!(date_ahead_of(no_date, received_epoch(no_date)), 0);
        let no_received = b"Date: Thu, 12 Nov 2026 18:40:32 +0000\r\n\r\n";
        assert_eq!(date_ahead_of(no_received, received_epoch(no_received)), 0);
        let unparsable = b"Received: x; whenever\r\nDate: soon\r\n\r\n";
        assert_eq!(date_ahead_of(unparsable, received_epoch(unparsable)), 0);
    }

    #[test]
    fn a_date_header_in_the_body_is_not_read() {
        let raw = b"Received: x; Mon, 12 Oct 2026 18:40:33 +0000\r\n\r\nDate: Thu, 12 Nov 2026 18:40:32 +0000\r\n";
        assert_eq!(date_header_epoch(raw), None);
    }

    #[test]
    fn a_wrong_weekday_and_a_comment_still_parse() {
        assert_eq!(
            parse_date("Fri, 12 Oct 2026 18:40:33 +0000 (UTC)"),
            Some(1_791_830_433)
        );
    }

    // 2026-07-02 12:00:00 UTC, written in zones the misdated mail used
    const NOON_UTC: i64 = 1_782_993_600;

    #[test]
    fn a_zone_east_of_utc_is_subtracted() {
        assert_eq!(parse_date("Thu, 2 Jul 2026 21:00:00 +0900"), Some(NOON_UTC));
    }

    #[test]
    fn zones_west_of_utc_are_added() {
        assert_eq!(parse_date("Thu, 2 Jul 2026 05:00:00 -0700"), Some(NOON_UTC));
        assert_eq!(
            parse_date("Thu, 2 Jul 2026 07:00:00 -0500 (CDT)"),
            Some(NOON_UTC)
        );
    }

    #[test]
    fn the_zone_is_read_off_the_header() {
        let tokyo = b"Date: Thu, 2 Jul 2026 21:00:00 +0900\r\n\r\n";
        assert_eq!(date_header_zone(tokyo), Some(Zone::Offset(9 * 3600)));
        let denver = b"Date: Thu, 2 Jul 2026 05:00:00 -0700\r\n\r\n";
        assert_eq!(date_header_zone(denver), Some(Zone::Offset(-7 * 3600)));
        let utc = b"Date: Thu, 2 Jul 2026 12:00:00 +0000\r\n\r\n";
        assert_eq!(date_header_zone(utc), Some(Zone::Offset(0)));
    }

    #[test]
    fn minus_zero_is_utc_with_no_known_zone() {
        let raw = b"Date: Thu, 2 Jul 2026 12:00:00 -0000\r\n\r\n";
        assert_eq!(date_header_epoch(raw), Some(NOON_UTC));
        assert_eq!(date_header_zone(raw), Some(Zone::Unknown));
        assert_eq!(date_header_zone(b"Date: soon\r\n\r\n"), None);
    }
}
