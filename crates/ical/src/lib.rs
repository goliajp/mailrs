
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
    Request,
    Reply,
    Cancel,
    Update,
    Counter,
    Refresh,
    Add,
    Publish,
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
        tz_name: String,
        local: chrono::NaiveDateTime,
    },
    /// Date-only (RFC 5545 §3.3.4). e.g. `DTSTART;VALUE=DATE:19980118`.
    Date(chrono::NaiveDate),
}

/// PARTSTAT parameter (RFC 5545 §3.2.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PartStat {
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
    Delegated,
    Completed,
    InProcess,
}

/// ROLE parameter (RFC 5545 §3.2.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Role {
    Chair,
    ReqParticipant,
    OptParticipant,
    NonParticipant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attendee {
    pub email: String,
    pub cn: Option<String>,
    pub partstat: PartStat,
    pub role: Role,
    pub rsvp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub email: String,
    pub cn: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EventStatus {
    Confirmed,
    Tentative,
    Cancelled,
}

/// VTIMEZONE component (RFC 5545 §3.6.5).
///
/// Self-built: STANDARD / DAYLIGHT children captured raw; conversion to a
/// usable offset function lives in [`vtimezone`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VTimezone {
    pub tzid: String,
    /// Raw STANDARD / DAYLIGHT subcomponents. Resolution to chrono-tz or
    /// custom offset happens lazily at evaluation time.
    pub raw_subs: Vec<RawComponent>,
}

/// Generic raw component captured by the AST parser before semantic typing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawComponent {
    pub name: String,
    pub properties: Vec<RawProperty>,
    pub children: Vec<RawComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RawProperty {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

/// Fully-typed iTIP invite, the boundary between this module and the rest of
/// the server (MRS-3..MRS-9 all consume this).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedInvite {
    pub method: Method,
    pub uid: String,
    pub sequence: i32,
    pub dtstamp: DateTime<Utc>,
    pub dtstart: CalDateTime,
    pub dtend: Option<CalDateTime>,
    pub duration: Option<chrono::Duration>,
    pub organizer: Option<Person>,
    pub attendees: Vec<Attendee>,
    /// Raw RRULE string (e.g. `FREQ=WEEKLY;BYDAY=MO,WE,FR`). Expansion is
    /// delegated to the `rrule` crate at MRS-9 time, not done here.
    pub rrule: Option<String>,
    pub exdate: Vec<CalDateTime>,
    pub rdate: Vec<CalDateTime>,
    pub recurrence_id: Option<CalDateTime>,
    pub status: Option<EventStatus>,
    pub summary: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub vtimezones: Vec<VTimezone>,
}

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
