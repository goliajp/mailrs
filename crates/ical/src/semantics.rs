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
//! TZID resolution against inline VTIMEZONE blocks is intentionally deferred
//! to [`super::vtimezone`]: this layer captures the TZID name verbatim into
//! [`super::CalDateTime::Zoned`] and the inline blocks into
//! [`super::ParsedInvite::vtimezones`].

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use super::{
    Attendee, CalDateTime, EventStatus, IcalError, Method, ParsedInvite, PartStat, Person,
    RawComponent, RawProperty, Role, VTimezone,
};

/// Lift a raw VCALENDAR component into a typed [`ParsedInvite`].
pub fn lift(calendar: &RawComponent) -> Result<ParsedInvite, IcalError> {
    if !calendar.name.eq_ignore_ascii_case("VCALENDAR") {
        return Err(IcalError::InvalidSemantics(format!(
            "top-level component is {}, expected VCALENDAR",
            calendar.name
        )));
    }

    // METHOD is at the VCALENDAR level (RFC 5546 §3.2).
    let method = match find_property(&calendar.properties, "METHOD") {
        Some(p) => parse_method(&p.value)?,
        None => Method::Publish,
    };

    // Pick the first VEVENT. RFC 5546 REPLY / CANCEL etc. carry exactly one
    // VEVENT for the targeted master / instance; multi-VEVENT calendars are
    // fine for PUBLISH but mailrs only treats the first as the iTIP payload.
    let vevent = calendar
        .children
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case("VEVENT"))
        .ok_or(IcalError::NoEvent)?;

    let uid = required_string(&vevent.properties, "UID")?;
    let sequence = optional_int(&vevent.properties, "SEQUENCE")?.unwrap_or(0);
    let dtstamp = required_utc(&vevent.properties, "DTSTAMP")?;
    let dtstart_prop = find_property(&vevent.properties, "DTSTART")
        .ok_or_else(|| IcalError::InvalidSemantics("VEVENT missing DTSTART".into()))?;
    let dtstart = parse_caldatetime(dtstart_prop)?;
    let dtend = find_property(&vevent.properties, "DTEND")
        .map(parse_caldatetime)
        .transpose()?;
    let duration = find_property(&vevent.properties, "DURATION")
        .map(|p| parse_duration(&p.value))
        .transpose()?;
    let summary = optional_string(&vevent.properties, "SUMMARY").unwrap_or_default();
    let location = optional_string(&vevent.properties, "LOCATION");
    let description = optional_string(&vevent.properties, "DESCRIPTION");

    let organizer = find_property(&vevent.properties, "ORGANIZER")
        .map(parse_person)
        .transpose()?;

    let mut attendees = Vec::new();
    for prop in vevent
        .properties
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("ATTENDEE"))
    {
        attendees.push(parse_attendee(prop)?);
    }

    // RRULE is captured raw — expansion happens in MRS-9 via the rrule crate.
    let rrule = find_property(&vevent.properties, "RRULE").map(|p| p.value.clone());

    let mut exdate = Vec::new();
    for prop in vevent
        .properties
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("EXDATE"))
    {
        for v in prop.value.split(',') {
            exdate.push(parse_caldatetime_with_params(v, &prop.params)?);
        }
    }

    let mut rdate = Vec::new();
    for prop in vevent
        .properties
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case("RDATE"))
    {
        for v in prop.value.split(',') {
            rdate.push(parse_caldatetime_with_params(v, &prop.params)?);
        }
    }

    let recurrence_id = find_property(&vevent.properties, "RECURRENCE-ID")
        .map(parse_caldatetime)
        .transpose()?;

    let status = optional_string(&vevent.properties, "STATUS")
        .as_deref()
        .map(parse_status)
        .transpose()?;

    let vtimezones = calendar
        .children
        .iter()
        .filter(|c| c.name.eq_ignore_ascii_case("VTIMEZONE"))
        .map(parse_vtimezone)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ParsedInvite {
        method,
        uid,
        sequence,
        dtstamp,
        dtstart,
        dtend,
        duration,
        organizer,
        attendees,
        rrule,
        exdate,
        rdate,
        recurrence_id,
        status,
        summary: unescape_text(&summary),
        location: location.as_deref().map(unescape_text),
        description: description.as_deref().map(unescape_text),
        vtimezones,
    })
}

fn find_property<'a>(props: &'a [RawProperty], name: &str) -> Option<&'a RawProperty> {
    props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

fn required_string(props: &[RawProperty], name: &str) -> Result<String, IcalError> {
    find_property(props, name)
        .map(|p| p.value.clone())
        .ok_or_else(|| IcalError::InvalidSemantics(format!("VEVENT missing {name}")))
}

fn optional_string(props: &[RawProperty], name: &str) -> Option<String> {
    find_property(props, name).map(|p| p.value.clone())
}

fn optional_int(props: &[RawProperty], name: &str) -> Result<Option<i32>, IcalError> {
    match find_property(props, name) {
        None => Ok(None),
        Some(p) => p.value.trim().parse::<i32>().map(Some).map_err(|_| {
            IcalError::InvalidSemantics(format!("{name} is not an integer: {}", p.value))
        }),
    }
}

fn required_utc(props: &[RawProperty], name: &str) -> Result<DateTime<Utc>, IcalError> {
    let prop = find_property(props, name)
        .ok_or_else(|| IcalError::InvalidSemantics(format!("VEVENT missing {name}")))?;
    parse_utc_only(&prop.value, name)
}

fn parse_method(s: &str) -> Result<Method, IcalError> {
    match s.trim().to_ascii_uppercase().as_str() {
        "REQUEST" => Ok(Method::Request),
        "REPLY" => Ok(Method::Reply),
        "CANCEL" => Ok(Method::Cancel),
        "PUBLISH" => Ok(Method::Publish),
        "ADD" => Ok(Method::Add),
        "REFRESH" => Ok(Method::Refresh),
        "COUNTER" => Ok(Method::Counter),
        "DECLINECOUNTER" => Ok(Method::DeclineCounter),
        // RFC 5546 §1.4 doesn't define UPDATE as a method name — Outlook /
        // Exchange use METHOD=REQUEST + a higher SEQUENCE for updates. But
        // some servers do emit METHOD=UPDATE; accept it for robustness.
        "UPDATE" => Ok(Method::Update),
        other => Err(IcalError::InvalidSemantics(format!(
            "unknown METHOD: {other}"
        ))),
    }
}

fn parse_partstat(s: &str) -> PartStat {
    match s.to_ascii_uppercase().as_str() {
        "ACCEPTED" => PartStat::Accepted,
        "DECLINED" => PartStat::Declined,
        "TENTATIVE" => PartStat::Tentative,
        "DELEGATED" => PartStat::Delegated,
        "COMPLETED" => PartStat::Completed,
        "IN-PROCESS" => PartStat::InProcess,
        // NEEDS-ACTION + anything unknown defaults per RFC 5545 §3.2.12.
        _ => PartStat::NeedsAction,
    }
}

fn parse_role(s: &str) -> Role {
    match s.to_ascii_uppercase().as_str() {
        "CHAIR" => Role::Chair,
        "OPT-PARTICIPANT" => Role::OptParticipant,
        "NON-PARTICIPANT" => Role::NonParticipant,
        // REQ-PARTICIPANT + unknown default per RFC 5545 §3.2.16.
        _ => Role::ReqParticipant,
    }
}

fn parse_status(s: &str) -> Result<EventStatus, IcalError> {
    match s.to_ascii_uppercase().as_str() {
        "CONFIRMED" => Ok(EventStatus::Confirmed),
        "TENTATIVE" => Ok(EventStatus::Tentative),
        "CANCELLED" => Ok(EventStatus::Cancelled),
        other => Err(IcalError::InvalidSemantics(format!(
            "unknown STATUS: {other}"
        ))),
    }
}

/// Extract a CAL-ADDRESS (`mailto:user@host`) from an ORGANIZER / ATTENDEE
/// value, plus the optional `CN` parameter.
fn parse_person(prop: &RawProperty) -> Result<Person, IcalError> {
    let email = strip_mailto(&prop.value)?;
    let cn = param(&prop.params, "CN").cloned();
    Ok(Person { email, cn })
}

fn parse_attendee(prop: &RawProperty) -> Result<Attendee, IcalError> {
    let email = strip_mailto(&prop.value)?;
    let cn = param(&prop.params, "CN").cloned();
    let partstat = param(&prop.params, "PARTSTAT")
        .map(|s| parse_partstat(s))
        .unwrap_or(PartStat::NeedsAction);
    let role = param(&prop.params, "ROLE")
        .map(|s| parse_role(s))
        .unwrap_or(Role::ReqParticipant);
    let rsvp = param(&prop.params, "RSVP")
        .map(|s| s.eq_ignore_ascii_case("TRUE"))
        .unwrap_or(false);
    Ok(Attendee {
        email,
        cn,
        partstat,
        role,
        rsvp,
    })
}

fn param<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a String> {
    params
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

fn strip_mailto(value: &str) -> Result<String, IcalError> {
    // CAL-ADDRESS may be `mailto:` (case-insensitive) or a bare URI.
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("mailto:") {
        return Ok(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("MAILTO:") {
        return Ok(rest.to_string());
    }
    // Tolerate raw email addresses (some buggy producers).
    Ok(trimmed.to_string())
}

/// Parse a property whose value is a date-time, choosing the variant by the
/// surrounding context (`Z` suffix → UTC; `VALUE=DATE` → date; TZID param →
/// zoned; otherwise floating).
fn parse_caldatetime(prop: &RawProperty) -> Result<CalDateTime, IcalError> {
    parse_caldatetime_with_params(&prop.value, &prop.params)
}

fn parse_caldatetime_with_params(
    value: &str,
    params: &[(String, String)],
) -> Result<CalDateTime, IcalError> {
    let value = value.trim();

    // VALUE=DATE forces a calendar-day-only interpretation.
    if param(params, "VALUE")
        .map(|v| v.eq_ignore_ascii_case("DATE"))
        .unwrap_or(false)
    {
        return Ok(CalDateTime::Date(parse_date(value)?));
    }

    // UTC: trailing 'Z'.
    if let Some(stripped) = value.strip_suffix('Z') {
        let naive = parse_naive_datetime(stripped)?;
        return Ok(CalDateTime::Utc(
            DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc),
        ));
    }

    // TZID-qualified.
    if let Some(tz_name) = param(params, "TZID") {
        let local = parse_naive_datetime(value)?;
        return Ok(CalDateTime::Zoned {
            tz_name: tz_name.clone(),
            local,
        });
    }

    // Floating local time (no Z, no TZID).
    Ok(CalDateTime::Floating(parse_naive_datetime(value)?))
}

fn parse_utc_only(value: &str, field: &str) -> Result<DateTime<Utc>, IcalError> {
    let value = value.trim();
    let stripped = value.strip_suffix('Z').ok_or_else(|| {
        IcalError::InvalidSemantics(format!("{field} must be UTC (trailing 'Z'), got: {value}"))
    })?;
    let naive = parse_naive_datetime(stripped)?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

/// Parse `YYYYMMDDTHHMMSS` (no timezone suffix; caller decides Utc vs Naive).
fn parse_naive_datetime(s: &str) -> Result<NaiveDateTime, IcalError> {
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
        .map_err(|e| IcalError::InvalidSemantics(format!("bad date-time {s}: {e}")))
}

/// Parse `YYYYMMDD`.
fn parse_date(s: &str) -> Result<NaiveDate, IcalError> {
    NaiveDate::parse_from_str(s, "%Y%m%d")
        .map_err(|e| IcalError::InvalidSemantics(format!("bad date {s}: {e}")))
}

/// Parse a duration string (RFC 5545 §3.3.6, e.g. `PT1H30M`).
///
/// First-cut: handle the most common forms (`PnW`, `PnDTnHnMnS`, `PTnHnMnS`,
/// optional leading `+` / `-`). Marginal corners (`P0D` etc.) fall through to
/// the explicit-token loop. Returns `chrono::Duration` directly.
fn parse_duration(s: &str) -> Result<chrono::Duration, IcalError> {
    let s = s.trim();
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1i64, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (1i64, r)
    } else {
        (1i64, s)
    };
    let rest = rest
        .strip_prefix('P')
        .ok_or_else(|| IcalError::InvalidSemantics(format!("DURATION must start with 'P': {s}")))?;

    let mut weeks = 0i64;
    let mut days = 0i64;
    let mut hours = 0i64;
    let mut minutes = 0i64;
    let mut seconds = 0i64;

    let mut in_time = false;
    let mut num = String::new();
    for ch in rest.chars() {
        if ch == 'T' {
            in_time = true;
            continue;
        }
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        let n: i64 = num
            .parse()
            .map_err(|_| IcalError::InvalidSemantics(format!("DURATION number: {s}")))?;
        num.clear();
        match (ch, in_time) {
            ('W', false) => weeks = n,
            ('D', false) => days = n,
            ('H', true) => hours = n,
            ('M', true) => minutes = n,
            ('S', true) => seconds = n,
            _ => {
                return Err(IcalError::InvalidSemantics(format!(
                    "DURATION unexpected token '{ch}' in {s}"
                )));
            }
        }
    }

    let total_seconds = (((weeks * 7 + days) * 24 + hours) * 60 + minutes) * 60 + seconds;
    Ok(chrono::Duration::seconds(sign * total_seconds))
}

/// Unescape a TEXT value (RFC 5545 §3.3.11).
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Capture VTIMEZONE with raw STANDARD / DAYLIGHT subcomponents preserved.
fn parse_vtimezone(comp: &RawComponent) -> Result<VTimezone, IcalError> {
    let tzid = find_property(&comp.properties, "TZID")
        .map(|p| compact_str::CompactString::new(&p.value))
        .ok_or_else(|| IcalError::InvalidSemantics("VTIMEZONE missing TZID".into()))?;
    let raw_subs = comp.children.clone();
    Ok(VTimezone { tzid, raw_subs })
}
