//! When an event actually is, as an instant.
//!
//! A `DTSTART` is not a moment until its zone is resolved, and resolving
//! it means walking the `VTIMEZONE` the organiser attached — an RRULE
//! transition table, not a name lookup. That work belongs on this side:
//! a browser handed `20260820T160000` and the string
//! `Pacific Standard Time` has no honest way to turn it into a time,
//! and the one dishonest way — read it as UTC — is what the web client
//! did, putting an afternoon meeting in Santa Clara at one in the
//! morning in Tokyo.
//!
//! All-day events are deliberately **not** resolved. A date has no
//! offset; giving it one is how an all-day event lands on the wrong
//! day for readers west of the organiser.

use chrono::{DateTime, Duration, Utc};

use crate::vtimezone::{local_to_utc_offset_seconds, resolve};
use crate::{CalDateTime, ParsedInvite};

/// The instant a calendar date-time names, or `None` when it names no
/// single instant — an all-day date, or a zone that resolves to nothing.
///
/// A floating time (no zone at all, RFC 5545 §3.3.5) means "local to
/// whoever is reading", so it has no instant either and is left to the
/// reader.
pub fn instant_of(dt: &CalDateTime, zones: &[crate::VTimezone]) -> Option<DateTime<Utc>> {
    match dt {
        CalDateTime::Utc(t) => Some(*t),
        CalDateTime::Zoned { tz_name, local } => {
            let rt = resolve(tz_name, zones)?;
            let offset = local_to_utc_offset_seconds(&rt, *local)?;
            Some(DateTime::from_naive_utc_and_offset(
                *local - Duration::seconds(offset as i64),
                Utc,
            ))
        }
        CalDateTime::Floating(_) | CalDateTime::Date(_) => None,
    }
}

/// Start and end of an invitation as instants, resolved through its own
/// `VTIMEZONE` blocks.
///
/// The end falls back to `DTSTART + DURATION` when the producer sent a
/// duration instead of a `DTEND`, which RFC 5545 allows and Zoom does.
pub fn instants_of(invite: &ParsedInvite) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let start = instant_of(&invite.dtstart, &invite.vtimezones);
    let end = match (&invite.dtend, invite.duration) {
        (Some(dtend), _) => instant_of(dtend, &invite.vtimezones),
        (None, Some(d)) => start.map(|s| s + d),
        (None, None) => None,
    };
    (start, end)
}

/// The link a meeting is actually joined by.
///
/// RFC 5545 has no field for it, so producers put it wherever: Teams
/// writes it into the description and names a physical room in
/// `LOCATION`, Zoom puts the URL in `LOCATION` itself. Reading only one
/// of the two finds a room in Santa Clara and no way to attend from
/// Tokyo.
///
/// Resolved here rather than in each client, because three
/// implementations of "which URL is a meeting" is three chances to
/// offer a button that goes somewhere else — and not every `https://`
/// in a mail body is a way in. Only hosts that are conferencing
/// services count.
pub fn join_link(invite: &ParsedInvite) -> Option<String> {
    const HOSTS: [&str; 5] = [
        "teams.microsoft.com",
        "zoom.us",
        "meet.google.com",
        "webex.com",
        "whereby.com",
    ];
    for field in [invite.location.as_deref(), invite.description.as_deref()] {
        let Some(text) = field else { continue };
        for token in text.split([' ', '\t', '\r', '\n', '<', '>', '"']) {
            let trimmed = token.trim_end_matches(['.', ',', ')', ']']);
            if !trimmed.starts_with("https://") {
                continue;
            }
            let host = trimmed
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap_or_default();
            if HOSTS
                .iter()
                .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
