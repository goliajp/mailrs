#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! RFC 5545 (iCalendar) + RFC 5546 (iTIP) parser, serializer, and typed
//! semantics — hand-rolled, zero I/O.
//!
//! Built for Rust MTAs that need to read an `iCalendar` invite off the wire
//! (typically a `text/calendar` MIME part) and emit a `REPLY` back. The
//! parser is byte-by-byte with no parser-combinator dependencies — the same
//! style as [`mailrs-smtp-proto`] and [`mailrs-imap-proto`] — keeping the
//! dependency footprint small and the error surface predictable.
//!
//! # Quick start
//!
//! ```
//! use mailrs_ical::{parse_invite, Method};
//!
//! let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\n\
//!             PRODID:-//Example//Cal//EN\r\nBEGIN:VEVENT\r\n\
//!             UID:abc\r\nDTSTAMP:20260101T000000Z\r\n\
//!             DTSTART:20260102T100000Z\r\nSUMMARY:Lunch\r\n\
//!             END:VEVENT\r\nEND:VCALENDAR\r\n";
//!
//! let invite = parse_invite(ics).unwrap();
//! assert_eq!(invite.method, Method::Request);
//! assert_eq!(invite.uid, "abc");
//! assert_eq!(invite.summary, "Lunch");
//! ```
//!
//! # Module layout
//!
//! - [`parse`]      — RFC 5545 §3.1 text → raw AST (line folding, property tree, BEGIN/END nesting).
//! - [`semantics`]  — AST → [`ParsedInvite`] (typed METHOD / ATTENDEE / ORGANIZER / SEQUENCE / RRULE / …).
//! - [`vtimezone`]  — Inline VTIMEZONE handling with `chrono-tz` IANA fallback.
//! - [`serialize`]  — [`ParsedInvite`] → RFC 5545 text (for iTIP `REPLY`).
//!
//! Top-level entry point [`parse_invite`] takes raw `text/calendar` bytes and
//! returns a fully-typed [`ParsedInvite`].
//!
//! # What this crate does NOT do
//!
//! - No MIME parsing — extract the `text/calendar` part upstream (e.g. with
//!   [`mail-parser`](https://crates.io/crates/mail-parser)).
//! - No SMTP. See [`mailrs-smtp-proto`] / [`mailrs-smtp-client`].
//! - No calendar storage or CalDAV. This is the wire-format layer only.
//!
//! [`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
//! [`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
//! [`mailrs-imap-proto`]: https://crates.io/crates/mailrs-imap-proto

pub mod parse;
pub mod semantics;
#[allow(clippy::module_inception)]
pub mod serialize;
pub mod vtimezone;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// iTIP method (RFC 5546 §1.4 + §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Method {
    /// `REQUEST` — invitation or update.
    Request,
    /// `REPLY` — attendee response (accept/decline/etc).
    Reply,
    /// `CANCEL` — organizer cancels the event.
    Cancel,
    /// `UPDATE` — non-significant update (no re-RSVP needed).
    Update,
    /// `COUNTER` — attendee proposes a change.
    Counter,
    /// `REFRESH` — attendee requests latest state.
    Refresh,
    /// `ADD` — add an occurrence to a recurring event.
    Add,
    /// `PUBLISH` — publish a non-interactive event (newsletter feed).
    Publish,
    /// `DECLINECOUNTER` — organizer rejects an attendee's COUNTER.
    DeclineCounter,
}

/// Calendar date-time tri-state (RFC 5545 §3.3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CalDateTime {
    /// Floating local time — no timezone attached. e.g. `DTSTART:19980118T230000`.
    Floating(chrono::NaiveDateTime),
    /// UTC. e.g. `DTSTART:19980119T070000Z`.
    Utc(DateTime<Utc>),
    /// TZID-qualified. e.g. `DTSTART;TZID=America/New_York:19980119T020000`.
    /// `tz_name` is the raw TZID string; resolved at evaluation time via
    /// [`vtimezone`] (handles both IANA names and inline VTIMEZONE blocks).
    Zoned {
        /// IANA timezone identifier or inline VTIMEZONE id.
        tz_name: String,
        /// Local civil time in that zone.
        local: chrono::NaiveDateTime,
    },
    /// Date-only (RFC 5545 §3.3.4). e.g. `DTSTART;VALUE=DATE:19980118`.
    Date(chrono::NaiveDate),
}

/// PARTSTAT parameter (RFC 5545 §3.2.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PartStat {
    /// `NEEDS-ACTION` — not yet responded.
    NeedsAction,
    /// `ACCEPTED` — will attend.
    Accepted,
    /// `DECLINED` — will not attend.
    Declined,
    /// `TENTATIVE` — may attend.
    Tentative,
    /// `DELEGATED` — passed to another attendee.
    Delegated,
    /// `COMPLETED` — VTODO only.
    Completed,
    /// `IN-PROCESS` — VTODO only.
    InProcess,
}

/// ROLE parameter (RFC 5545 §3.2.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Role {
    /// `CHAIR` — meeting chair.
    Chair,
    /// `REQ-PARTICIPANT` — required attendance.
    ReqParticipant,
    /// `OPT-PARTICIPANT` — optional attendance.
    OptParticipant,
    /// `NON-PARTICIPANT` — for-information-only.
    NonParticipant,
}

/// One ATTENDEE row from a VEVENT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attendee {
    /// Mailto address (stripped of the `mailto:` prefix).
    pub email: String,
    /// Common name (`CN=` parameter), if present.
    pub cn: Option<String>,
    /// Response status.
    pub partstat: PartStat,
    /// Participation role.
    pub role: Role,
    /// `RSVP=TRUE` if the organizer wants an explicit response.
    pub rsvp: bool,
}

/// ORGANIZER or any other CAL-ADDRESS-shaped property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    /// Mailto address.
    pub email: String,
    /// Common name (`CN=` parameter).
    pub cn: Option<String>,
}

/// STATUS property values for a VEVENT (RFC 5545 §3.8.1.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EventStatus {
    /// `CONFIRMED` — event is confirmed.
    Confirmed,
    /// `TENTATIVE` — event is tentative.
    Tentative,
    /// `CANCELLED` — event is cancelled.
    Cancelled,
}

/// VTIMEZONE component (RFC 5545 §3.6.5).
///
/// Self-built: STANDARD / DAYLIGHT children captured raw; conversion to a
/// usable offset function lives in [`vtimezone`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VTimezone {
    /// TZID property — the timezone identifier this block defines.
    pub tzid: String,
    /// Raw STANDARD / DAYLIGHT subcomponents. Resolution to chrono-tz or
    /// custom offset happens lazily at evaluation time.
    pub raw_subs: Vec<RawComponent>,
}

/// Generic raw component captured by the AST parser before semantic typing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawComponent {
    /// Component name (e.g. `VEVENT`, `VALARM`, `STANDARD`).
    pub name: String,
    /// Properties on this component.
    pub properties: Vec<RawProperty>,
    /// Nested subcomponents (e.g. `VALARM` inside `VEVENT`).
    pub children: Vec<RawComponent>,
}

/// Single iCalendar property with its value + parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawProperty {
    /// Property name (e.g. `DTSTART`, `SUMMARY`, `ATTENDEE`).
    pub name: String,
    /// Parameter list (e.g. `TZID=America/New_York`).
    pub params: Vec<(String, String)>,
    /// Property value string (un-unfolded).
    pub value: String,
}

/// Fully-typed iTIP invite, the boundary between this module and the rest of
/// the server (MRS-3..MRS-9 all consume this).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedInvite {
    /// iTIP method (REQUEST/REPLY/CANCEL/...).
    pub method: Method,
    /// `UID` — RFC 5545 §3.8.4.7.
    pub uid: String,
    /// `SEQUENCE` — incremented on each update.
    pub sequence: i32,
    /// `DTSTAMP` — when the iTIP message was created.
    pub dtstamp: DateTime<Utc>,
    /// `DTSTART` — event start.
    pub dtstart: CalDateTime,
    /// `DTEND` — event end (mutually exclusive with `duration`).
    pub dtend: Option<CalDateTime>,
    /// `DURATION` — alternative to `DTEND`.
    pub duration: Option<chrono::Duration>,
    /// `ORGANIZER` — event chair.
    pub organizer: Option<Person>,
    /// `ATTENDEE` list.
    pub attendees: Vec<Attendee>,
    /// Raw RRULE string (e.g. `FREQ=WEEKLY;BYDAY=MO,WE,FR`). Expansion is
    /// delegated to the `rrule` crate at MRS-9 time, not done here.
    pub rrule: Option<String>,
    /// `EXDATE` — explicit exclusions from the recurrence rule.
    pub exdate: Vec<CalDateTime>,
    /// `RDATE` — explicit additions to the recurrence set.
    pub rdate: Vec<CalDateTime>,
    /// `RECURRENCE-ID` — this iTIP message modifies a specific occurrence.
    pub recurrence_id: Option<CalDateTime>,
    /// `STATUS` — CONFIRMED / TENTATIVE / CANCELLED.
    pub status: Option<EventStatus>,
    /// `SUMMARY` — short title shown in calendar UIs.
    pub summary: String,
    /// `LOCATION` — free-form location text.
    pub location: Option<String>,
    /// `DESCRIPTION` — long-form body / notes.
    pub description: Option<String>,
    /// `VTIMEZONE` blocks attached to the calendar; referenced by `TZID` in
    /// other properties.
    pub vtimezones: Vec<VTimezone>,
}

/// Errors returned by [`parse_invite`].
#[derive(Debug, PartialEq, Eq)]
pub enum IcalError {
    /// Input bytes were not valid UTF-8 (RFC 5545 §3.1.4 mandates UTF-8).
    NotUtf8,
    /// Lexer / property-tree level failure.
    InvalidSyntax(String),
    /// AST is well-formed but semantic typing failed (e.g. missing UID, bad METHOD).
    InvalidSemantics(String),
    /// No VEVENT component found in the VCALENDAR.
    NoEvent,
}

/// Top-level entry: raw `text/calendar` bytes → fully-typed invite.
///
/// Pipeline: bytes → UTF-8 → [`parse::parse_calendar`] (AST) → [`semantics::lift`] (ParsedInvite).
pub fn parse_invite(bytes: &[u8]) -> Result<ParsedInvite, IcalError> {
    let text = std::str::from_utf8(bytes).map_err(|_| IcalError::NotUtf8)?;
    let calendar = parse::parse_calendar(text)?;
    semantics::lift(&calendar)
}
