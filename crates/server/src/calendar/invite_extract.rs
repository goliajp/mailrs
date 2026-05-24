//! Locate the `text/calendar` MIME part in an inbound message.
//!
//! Real-world iTIP / iMIP invitations land in two shapes:
//! 1. **Inline** (RFC 6047 §2.1 recommended): `multipart/alternative` with
//!    `text/plain`, `text/html`, and `text/calendar; method=REQUEST` parts.
//!    Common for Apple iCal, Google Calendar.
//! 2. **Attachment** (de-facto Outlook / Teams): the meeting invite is sent
//!    as a `text/calendar` attachment named `invite.ics` or `unnamed`.
//!
//! This module walks the parsed MIME tree once and returns the first
//! `text/calendar` part it finds, regardless of disposition. The caller
//! (`mailrs_ical::parse_invite`) then validates the bytes are RFC 5545
//! conformant.

/// What the inbound pipeline gets back when it finds a calendar part.
pub struct ExtractedInvite {
    /// Raw `text/calendar` body bytes (post-MIME-decode).
    pub ics_bytes: Vec<u8>,
    /// METHOD parameter from the Content-Type header, if present
    /// (e.g. `REQUEST`, `REPLY`, `CANCEL`). Empty when absent — some
    /// producers omit and stash the method only inside the iCalendar
    /// body. MRS-7 (state machine) cross-checks this against the body
    /// METHOD for tampering detection; left unread today.
    #[allow(dead_code)]
    pub content_type_method: String,
}

/// Scan a parsed message for a `text/calendar` part. Returns the first
/// match. None when no such part exists.
///
/// Now backed by `mailrs-mime` (deps-audit #2 stone); previously used
/// `mail-parser`. Behavior identical.
pub fn extract_invite_part(data: &[u8]) -> Option<ExtractedInvite> {
    let root = mailrs_mime::parse(data);
    let part = root.find_by_content_type("text/calendar")?;
    let content_type_method = part
        .content_type
        .params
        .get("method")
        .cloned()
        .unwrap_or_default()
        .to_ascii_uppercase();
    Some(ExtractedInvite {
        ics_bytes: part.body.clone(),
        content_type_method,
    })
}

#[cfg(test)]
mod tests {
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
}
