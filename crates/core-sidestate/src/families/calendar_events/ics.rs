//! iCalendar text in and out of a [`StoredEvent`], for CalDAV.
//!
//! In: the columns are read out of the client's text the way the feed sync
//! reads a feed — through `mailrs-ical`, with zoned times resolved — and the
//! text itself is kept, so a GET returns what the PUT sent. Out: a row a
//! feed or a mail produced has no text, so one is written from the columns.

use mailrs_ical::vtimezone::caldatetime_to_utc;
use mailrs_ical::{EventStatus, parse_invite};

use super::StoredEvent;

/// The row a CalDAV PUT of `ics` to `uid` stores.
///
/// `Err` says why the text cannot be stored: it does not parse, or its
/// start names a zone nothing resolves, which would leave an event that
/// cannot be placed on any calendar.
pub fn from_ics(uid: &str, ics: &str, source: &str) -> Result<StoredEvent, String> {
    let parsed = parse_invite(ics.as_bytes()).map_err(|e| format!("{e:?}"))?;
    let start = caldatetime_to_utc(&parsed.dtstart, &parsed.vtimezones)
        .ok_or_else(|| "DTSTART names a time zone that cannot be resolved".to_string())?;
    let end = match (&parsed.dtend, parsed.duration) {
        (Some(end), _) => caldatetime_to_utc(end, &parsed.vtimezones),
        (None, Some(duration)) => Some(start + duration),
        (None, None) => None,
    };
    let status = parsed.status.map(|s| match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    });
    Ok(StoredEvent {
        uid: uid.to_string(),
        summary: parsed.summary,
        dtstart: Some(start.to_rfc3339()),
        dtend: end.map(|e| e.to_rfc3339()),
        organizer: parsed.organizer.map(|o| o.email),
        status: status.map(str::to_string),
        source: source.to_string(),
        sequence: parsed.sequence,
        ics: Some(ics.to_string()),
    })
}

/// The iCalendar text CalDAV serves for `ev`.
///
/// Deterministic, because the ETag is a digest of it: the same row must
/// always produce the same bytes. `DTSTAMP` is required by RFC 5545 and
/// the row has no creation time, so the start stands in for it.
pub fn to_ics(ev: &StoredEvent) -> String {
    if let Some(raw) = &ev.ics {
        return raw.clone();
    }
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//mailrs//calendar//EN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", escape(&ev.uid)),
        format!("SEQUENCE:{}", ev.sequence),
    ];
    let start = ev.dtstart.as_deref().and_then(utc_basic);
    lines.push(format!(
        "DTSTAMP:{}",
        start.as_deref().unwrap_or("19700101T000000Z")
    ));
    if let Some(s) = &start {
        lines.push(format!("DTSTART:{s}"));
    }
    if let Some(e) = ev.dtend.as_deref().and_then(utc_basic) {
        lines.push(format!("DTEND:{e}"));
    }
    lines.push(format!("SUMMARY:{}", escape(&ev.summary)));
    if let Some(o) = &ev.organizer {
        lines.push(format!("ORGANIZER:mailto:{o}"));
    }
    if let Some(s) = &ev.status {
        lines.push(format!("STATUS:{s}"));
    }
    lines.push("END:VEVENT".to_string());
    lines.push("END:VCALENDAR".to_string());
    lines.join("\r\n") + "\r\n"
}

/// RFC 3339 → the RFC 5545 UTC form, `20260820T090000Z`.
fn utc_basic(rfc3339: &str) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    Some(
        t.with_timezone(&chrono::Utc)
            .format("%Y%m%dT%H%M%SZ")
            .to_string(),
    )
}

/// RFC 5545 §3.3.11 TEXT escaping.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\n\
        DTSTAMP:20260801T000000Z\r\nDTSTART;TZID=Asia/Tokyo:20260820T100000\r\n\
        DURATION:PT30M\r\nSUMMARY:Sync\r\nSTATUS:CONFIRMED\r\nSEQUENCE:2\r\n\
        END:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_zoned_start_resolves_and_the_text_is_kept() {
        let ev = from_ics("e1", ZONED, "caldav").expect("parses");
        assert_eq!(ev.dtstart.as_deref(), Some("2026-08-20T01:00:00+00:00"));
        assert_eq!(ev.dtend.as_deref(), Some("2026-08-20T01:30:00+00:00"));
        assert_eq!(ev.status.as_deref(), Some("CONFIRMED"));
        assert_eq!(ev.sequence, 2);
        assert_eq!(to_ics(&ev), ZONED);
    }

    #[test]
    fn text_that_is_not_an_event_is_refused() {
        assert!(from_ics("x", "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n", "caldav").is_err());
        assert!(from_ics("x", "not ical", "caldav").is_err());
    }

    /// A row with no stored text gets text from its columns, and that text
    /// reads back as the same row — what a CalDAV client edits is what
    /// the conflicts check saw.
    #[test]
    fn written_text_reads_back_as_the_same_columns() {
        let ev = StoredEvent {
            uid: "m1".into(),
            summary: "Plan, review; ship".into(),
            dtstart: Some("2026-08-20T09:00:00+00:00".into()),
            dtend: Some("2026-08-20T10:00:00+00:00".into()),
            organizer: Some("chair@example.com".into()),
            status: Some("TENTATIVE".into()),
            source: "mail:m1".into(),
            sequence: 3,
            ics: None,
        };
        let text = to_ics(&ev);
        assert_eq!(text, to_ics(&ev));
        let back = from_ics("m1", &text, "mail:m1").expect("parses");
        assert_eq!(back.summary, ev.summary);
        assert_eq!(back.dtstart, ev.dtstart);
        assert_eq!(back.dtend, ev.dtend);
        assert_eq!(back.organizer, ev.organizer);
        assert_eq!(back.status, ev.status);
        assert_eq!(back.sequence, ev.sequence);
    }
}
