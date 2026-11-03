//! What the `dtstart` / `dtend` columns hold. An event at a time of day
//! stores RFC 3339 instants. An all-day event stores dates, `YYYY-MM-DD`,
//! with the end exclusive: a date has no offset, and giving it one (UTC
//! midnight) put a one-day holiday across two days, starting at 09:00,
//! for a reader in Tokyo.

use chrono::NaiveDate;
use mailrs_ical::ParsedInvite;
use mailrs_ical::instants::all_day_of;
use mailrs_ical::vtimezone::caldatetime_to_utc;

const DATE: &str = "%Y-%m-%d";

/// The two columns for `parsed`. `None` when its start names a zone
/// nothing resolves: there is no honest place to put it.
pub fn span_of(parsed: &ParsedInvite) -> Option<(String, Option<String>)> {
    if let Some((start, end)) = all_day_of(parsed) {
        return Some((
            start.format(DATE).to_string(),
            Some(end.format(DATE).to_string()),
        ));
    }
    let start = caldatetime_to_utc(&parsed.dtstart, &parsed.vtimezones)?;
    let end = match (&parsed.dtend, parsed.duration) {
        (Some(end), _) => caldatetime_to_utc(end, &parsed.vtimezones),
        (None, Some(duration)) => Some(start + duration),
        (None, None) => None,
    };
    Some((start.to_rfc3339(), end.map(|e| e.to_rfc3339())))
}

/// The date a column holds, when it holds a date rather than an instant.
pub fn date_of(column: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(column, DATE).ok()
}

/// A column as epoch seconds, for ordering: the instant, or the date's
/// midnight in UTC.
pub fn epoch_of(column: &str) -> Option<i64> {
    if let Some(d) = date_of(column) {
        return d.and_hms_opt(0, 0, 0).map(|t| t.and_utc().timestamp());
    }
    chrono::DateTime::parse_from_rfc3339(column)
        .ok()
        .map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(lines: &str) -> ParsedInvite {
        let ics = format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u\r\nSUMMARY:s\r\nDTSTAMP:20261101T000000Z\r\n{lines}END:VEVENT\r\nEND:VCALENDAR\r\n"
        );
        mailrs_ical::parse_invite(ics.as_bytes()).expect("parse")
    }

    #[test]
    fn an_all_day_event_is_stored_as_dates_and_a_timed_one_as_instants() {
        assert_eq!(
            span_of(&parsed("DTSTART;VALUE=DATE:20261103\r\n")),
            Some(("2026-11-03".into(), Some("2026-11-04".into())))
        );
        assert_eq!(
            span_of(&parsed("DTSTART:20261103T010000Z\r\nDURATION:PT30M\r\n")),
            Some((
                "2026-11-03T01:00:00+00:00".into(),
                Some("2026-11-03T01:30:00+00:00".into())
            ))
        );
    }

    #[test]
    fn both_shapes_order_by_epoch() {
        assert_eq!(epoch_of("2026-11-03"), Some(1_793_664_000));
        assert_eq!(epoch_of("2026-11-03T01:00:00+00:00"), Some(1_793_667_600));
        assert_eq!(date_of("2026-11-03T01:00:00+00:00"), None);
    }
}
