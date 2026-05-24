//! Tests for `invite_extract` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

/// A multipart/alternative invite, Apple-Calendar-style, with the
/// `text/calendar` part inline.
const INLINE_INVITE: &[u8] = b"\
From: organizer@example.com\r\n\
To: attendee@example.com\r\n\
Subject: Project sync\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"BOUNDARY\"\r\n\
\r\n\
--BOUNDARY\r\n\
Content-Type: text/plain\r\n\
\r\n\
Plain text body.\r\n\
--BOUNDARY\r\n\
Content-Type: text/calendar; method=REQUEST; charset=UTF-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:test-uid\r\n\
DTSTAMP:20260430T120000Z\r\n\
DTSTART:20260501T140000Z\r\n\
SUMMARY:Project sync\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n\
--BOUNDARY--\r\n";

/// An Outlook-style invite where the .ics rides as an attachment.
const ATTACHMENT_INVITE: &[u8] = b"\
From: o@example.com\r\n\
To: a@example.com\r\n\
Subject: Meeting\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"X\"\r\n\
\r\n\
--X\r\n\
Content-Type: text/html\r\n\
\r\n\
<html><body>Click to join</body></html>\r\n\
--X\r\n\
Content-Type: text/calendar; method=REQUEST\r\n\
Content-Disposition: attachment; filename=\"invite.ics\"\r\n\
\r\n\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:att-uid\r\n\
DTSTAMP:20260430T120000Z\r\n\
DTSTART:20260501T140000Z\r\n\
SUMMARY:Att invite\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n\
--X--\r\n";

#[test]
fn finds_inline_calendar_part() {
    let extracted = extract_invite_part(INLINE_INVITE).expect("should find inline");
    assert_eq!(extracted.content_type_method, "REQUEST");
    let s = std::str::from_utf8(&extracted.ics_bytes).unwrap();
    assert!(s.contains("UID:test-uid"));
}

#[test]
fn finds_attachment_calendar_part() {
    let extracted = extract_invite_part(ATTACHMENT_INVITE).expect("should find attachment");
    assert_eq!(extracted.content_type_method, "REQUEST");
    let s = std::str::from_utf8(&extracted.ics_bytes).unwrap();
    assert!(s.contains("UID:att-uid"));
}

#[test]
fn returns_none_for_plain_text_email() {
    let plain: &[u8] = b"From: a@b.c\r\nTo: d@e.f\r\nSubject: hi\r\n\r\nNo invite here.\r\n";
    assert!(extract_invite_part(plain).is_none());
}
