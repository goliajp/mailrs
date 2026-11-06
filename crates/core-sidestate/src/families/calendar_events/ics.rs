//! iCalendar text in and out of a [`StoredEvent`], for CalDAV.
//!
//! In: the columns are read out of the client's text the way the feed sync
//! reads a feed — through `mailrs-ical`, with zoned times resolved — and the
//! text itself is kept, so a GET returns what the PUT sent. Out: a row a
//! feed or a mail produced has no text, so one is written from the columns.

use mailrs_ical::recur::Head;
use mailrs_ical::{EventStatus, parse_invite};

use super::StoredEvent;
use super::span::{date_of, span_of};

/// The row a CalDAV PUT of `ics` stores, keyed by the text's own `UID`.
///
/// `Err` says why the text cannot be stored: it does not parse, or its
/// start names a zone nothing resolves, which would leave an event that
/// cannot be placed on any calendar.
///
/// A recurring event comes as its series plus one `VEVENT` per changed
/// occurrence; the row is the series, with those folded in.
pub fn from_ics(ics: &str, source: &str) -> Result<StoredEvent, String> {
    let first = parse_invite(ics.as_bytes()).map_err(|e| format!("{e:?}"))?;
    let (parsed, recurrence) = super::series::fold_text(ics)
        .into_iter()
        .find(|(ev, _)| ev.uid == first.uid)
        .unwrap_or((first, None));
    let (start, end) = span_of(&parsed)
        .ok_or_else(|| "DTSTART names a time zone that cannot be resolved".to_string())?;
    let status = parsed.status.map(|s| match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    });
    Ok(StoredEvent {
        uid: parsed.uid,
        summary: parsed.summary,
        dtstart: Some(start),
        dtend: end,
        organizer: parsed.organizer.map(|o| o.email),
        status: status.map(str::to_string),
        source: source.to_string(),
        sequence: parsed.sequence,
        ics: Some(ics.to_string()),
        partstat: None,
        message_id: None,
        recurrence,
    })
}

/// The iCalendar text CalDAV serves for `ev`. A recurring row is written
/// as its whole series — rule, exclusions, additions, zone and moved
/// occurrences — so a client shows every occurrence, not the first.
///
/// Deterministic, because the ETag is a digest of it: the same row must
/// always produce the same bytes. `DTSTAMP` is required by RFC 5545 and
/// the row has no creation time, so the start stands in for it.
pub fn to_ics(ev: &StoredEvent) -> String {
    if let Some(raw) = &ev.ics {
        return raw.clone();
    }
    let stamp = ev
        .dtstart
        .as_deref()
        .and_then(utc_basic)
        .unwrap_or_else(|| "19700101T000000Z".to_string());
    let head = Head {
        uid: &ev.uid,
        summary: &ev.summary,
        organizer: ev.organizer.as_deref(),
        status: ev.status.as_deref(),
        sequence: ev.sequence,
        dtstamp: &stamp,
    };
    // a series whose zone no longer resolves still shows as its first
    // occurrence rather than not at all
    if let Some(text) = super::series::series_of(ev).and_then(|s| s.to_calendar(&head)) {
        return text;
    }
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//mailrs//calendar//EN".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", escape(&ev.uid)),
        format!("SEQUENCE:{}", ev.sequence),
    ];
    let start = ev.dtstart.as_deref().and_then(property_value);
    lines.push(format!("DTSTAMP:{stamp}"));
    if let Some(s) = &start {
        lines.push(format!("DTSTART{s}"));
    }
    if let Some(e) = ev.dtend.as_deref().and_then(property_value) {
        lines.push(format!("DTEND{e}"));
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

/// A stored column as what follows the property name: `;VALUE=DATE:20261103`
/// for a date, `:20260820T090000Z` for an instant.
fn property_value(column: &str) -> Option<String> {
    if let Some(d) = date_of(column) {
        return Some(format!(";VALUE=DATE:{}", d.format("%Y%m%d")));
    }
    utc_basic(column).map(|t| format!(":{t}"))
}

/// RFC 3339 → the RFC 5545 UTC form, `20260820T090000Z`. A date stands in
/// as its midnight.
fn utc_basic(rfc3339: &str) -> Option<String> {
    if let Some(d) = date_of(rfc3339) {
        return Some(format!("{}T000000Z", d.format("%Y%m%d")));
    }
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
        let ev = from_ics(ZONED, "caldav").expect("parses");
        assert_eq!(ev.uid, "e1", "keyed by the text's own UID");
        assert_eq!(ev.dtstart.as_deref(), Some("2026-08-20T01:00:00+00:00"));
        assert_eq!(ev.dtend.as_deref(), Some("2026-08-20T01:30:00+00:00"));
        assert_eq!(ev.status.as_deref(), Some("CONFIRMED"));
        assert_eq!(ev.sequence, 2);
        assert_eq!(to_ics(&ev), ZONED);
    }

    /// An all-day event is filed as its dates and written back as dates,
    /// not as an instant at UTC midnight.
    #[test]
    fn an_all_day_event_keeps_its_dates_both_ways() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:h1\r\n\
            DTSTAMP:20261101T000000Z\r\nDTSTART;VALUE=DATE:20261103\r\n\
            SUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ev = from_ics(text, "caldav").expect("parses");
        assert_eq!(ev.dtstart.as_deref(), Some("2026-11-03"));
        assert_eq!(ev.dtend.as_deref(), Some("2026-11-04"));
        let written = to_ics(&StoredEvent {
            ics: None,
            ..ev.clone()
        });
        assert!(
            written.contains("DTSTART;VALUE=DATE:20261103\r\n"),
            "{written}"
        );
        assert!(
            written.contains("DTEND;VALUE=DATE:20261104\r\n"),
            "{written}"
        );
        let back = from_ics(&written, "caldav").expect("parses");
        assert_eq!((back.dtstart, back.dtend), (ev.dtstart, ev.dtend));
    }

    #[test]
    fn text_that_is_not_an_event_is_refused() {
        assert!(from_ics("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n", "caldav").is_err());
        assert!(from_ics("not ical", "caldav").is_err());
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
            partstat: None,
            message_id: None,
            recurrence: None,
        };
        let text = to_ics(&ev);
        assert_eq!(text, to_ics(&ev));
        let back = from_ics(&text, "mail:m1").expect("parses");
        assert_eq!(back.uid, ev.uid);
        assert_eq!(back.summary, ev.summary);
        assert_eq!(back.dtstart, ev.dtstart);
        assert_eq!(back.dtend, ev.dtend);
        assert_eq!(back.organizer, ev.organizer);
        assert_eq!(back.status, ev.status);
        assert_eq!(back.sequence, ev.sequence);
    }
}
