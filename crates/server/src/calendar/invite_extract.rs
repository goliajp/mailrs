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
#[path = "invite_extract_tests.rs"]
mod tests;
