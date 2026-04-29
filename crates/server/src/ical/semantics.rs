//! AST → typed [`ParsedInvite`].
//!
//! Maps the raw component / property tree onto strongly-typed fields. Handles:
//! - METHOD on VCALENDAR
//! - VEVENT properties (UID / SEQUENCE / DTSTAMP / DTSTART / DTEND / DURATION /
//!   SUMMARY / LOCATION / DESCRIPTION / STATUS / RECURRENCE-ID)
//! - ATTENDEE list with PARTSTAT / RSVP / ROLE / CN parameters
//! - ORGANIZER with mailto + CN
//! - RRULE / EXDATE / RDATE (raw — RRULE expansion is done by the `rrule` crate)
//!
//! MRS-2 phase 1: signature only.

use super::{IcalError, ParsedInvite, RawComponent};

/// Lift a raw VCALENDAR component into a typed [`ParsedInvite`].
///
/// Errors with [`IcalError::NoEvent`] when the calendar contains no VEVENT,
/// or [`IcalError::InvalidSemantics`] when required fields are missing or
/// malformed.
pub fn lift(_calendar: &RawComponent) -> Result<ParsedInvite, IcalError> {
    Err(IcalError::InvalidSemantics(
        "semantics::lift not yet implemented".into(),
    ))
}
