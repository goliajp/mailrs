//! [`ParsedInvite`] → RFC 5545 text.
//!
//! Reverse of [`super::parse`] + [`super::semantics`]. Required for iTIP REPLY
//! generation in MRS-6 (where mailrs flips PARTSTAT and ships the modified
//! invite back to the ORGANIZER through the outbound queue).
//!
//! Output guarantees:
//! - strict line folding at 75 octets, CRLF + space continuation (§3.1)
//! - text escaping for SUMMARY / LOCATION / DESCRIPTION (§3.3.11)
//! - UID and SEQUENCE preserved byte-for-byte
//! - DTSTAMP serialized as the original UTC value (caller bumps if needed)
//!
//! Style: hand-rolled string assembly, no template / format library beyond
//! `chrono`'s strftime.
//!
//! Round-trip property: `parse → serialize → parse` yields the same
//! [`ParsedInvite`] (verified in `tests.rs`).

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Timelike, Utc};

use super::{
    Attendee, CalDateTime, EventStatus, IcalError, Method, ParsedInvite, PartStat, Person,
    RawComponent, RawProperty, Role, VTimezone,
};

/// Serialize a [`ParsedInvite`] to a complete VCALENDAR text suitable for
/// embedding as `text/calendar; method=<method>` in an iTIP MIME part.
pub fn serialize(invite: &ParsedInvite) -> Result<String, IcalError> {
    let mut out = String::new();

    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//mailrs//MRS-1 ICS//EN");
    push_line(&mut out, &format!("METHOD:{}", method_str(invite.method)));

    for tz in &invite.vtimezones {
        write_vtimezone(&mut out, tz);
    }

    push_line(&mut out, "BEGIN:VEVENT");
    push_line(&mut out, &format!("UID:{}", escape_text(&invite.uid)));
    push_line(&mut out, &format!("SEQUENCE:{}", invite.sequence));
    push_line(
        &mut out,
        &format!("DTSTAMP:{}", format_utc(&invite.dtstamp)),
    );
    write_caldatetime(&mut out, "DTSTART", &invite.dtstart);
    if let Some(end) = &invite.dtend {
        write_caldatetime(&mut out, "DTEND", end);
    }
    if let Some(d) = &invite.duration {
        push_line(&mut out, &format!("DURATION:{}", format_duration(d)));
    }
    if !invite.summary.is_empty() {
        push_line(
            &mut out,
            &format!("SUMMARY:{}", escape_text(&invite.summary)),
        );
    }
    if let Some(loc) = &invite.location {
        push_line(&mut out, &format!("LOCATION:{}", escape_text(loc)));
    }
    if let Some(desc) = &invite.description {
        push_line(&mut out, &format!("DESCRIPTION:{}", escape_text(desc)));
    }
    if let Some(status) = invite.status {
        push_line(&mut out, &format!("STATUS:{}", status_str(status)));
    }
    if let Some(rid) = &invite.recurrence_id {
        write_caldatetime(&mut out, "RECURRENCE-ID", rid);
    }
    if let Some(rrule) = &invite.rrule {
        push_line(&mut out, &format!("RRULE:{rrule}"));
    }
    for ex in &invite.exdate {
        write_caldatetime(&mut out, "EXDATE", ex);
    }
    for rd in &invite.rdate {
        write_caldatetime(&mut out, "RDATE", rd);
    }
    if let Some(org) = &invite.organizer {
        push_line(&mut out, &serialize_person("ORGANIZER", org));
    }
    for att in &invite.attendees {
        push_line(&mut out, &serialize_attendee(att));
    }
    push_line(&mut out, "END:VEVENT");
    push_line(&mut out, "END:VCALENDAR");

    Ok(out)
}

fn method_str(m: Method) -> &'static str {
    match m {
        Method::Request => "REQUEST",
        Method::Reply => "REPLY",
        Method::Cancel => "CANCEL",
        Method::Update => "UPDATE",
        Method::Counter => "COUNTER",
        Method::Refresh => "REFRESH",
        Method::Add => "ADD",
        Method::Publish => "PUBLISH",
        Method::DeclineCounter => "DECLINECOUNTER",
    }
}

fn status_str(s: EventStatus) -> &'static str {
    match s {
        EventStatus::Confirmed => "CONFIRMED",
        EventStatus::Tentative => "TENTATIVE",
        EventStatus::Cancelled => "CANCELLED",
    }
}

fn partstat_str(p: PartStat) -> &'static str {
    match p {
        PartStat::NeedsAction => "NEEDS-ACTION",
        PartStat::Accepted => "ACCEPTED",
        PartStat::Declined => "DECLINED",
        PartStat::Tentative => "TENTATIVE",
        PartStat::Delegated => "DELEGATED",
        PartStat::Completed => "COMPLETED",
        PartStat::InProcess => "IN-PROCESS",
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::Chair => "CHAIR",
        Role::ReqParticipant => "REQ-PARTICIPANT",
        Role::OptParticipant => "OPT-PARTICIPANT",
        Role::NonParticipant => "NON-PARTICIPANT",
    }
}

fn serialize_person(prop_name: &str, p: &Person) -> String {
    let cn_part = match &p.cn {
        Some(cn) => format!(";CN={}", quote_param_if_needed(cn)),
        None => String::new(),
    };
    format!("{prop_name}{cn_part}:mailto:{}", p.email)
}

fn serialize_attendee(a: &Attendee) -> String {
    let mut header = String::from("ATTENDEE");
    if let Some(cn) = &a.cn {
        header.push_str(&format!(";CN={}", quote_param_if_needed(cn)));
    }
    header.push_str(&format!(";PARTSTAT={}", partstat_str(a.partstat)));
    header.push_str(&format!(";ROLE={}", role_str(a.role)));
    header.push_str(&format!(";RSVP={}", if a.rsvp { "TRUE" } else { "FALSE" }));
    format!("{header}:mailto:{}", a.email)
}

/// Quote a parameter value if it contains characters that would otherwise
/// break the `[;param=value]` syntax (`:`, `;`, `,`, double-quote).
fn quote_param_if_needed(value: &str) -> String {
    let needs = value.contains([':', ';', ',', '"']);
    if needs {
        // RFC 5545 §3.2 disallows bare `"` inside quoted values; strip it.
        let cleaned = value.replace('"', "");
        format!("\"{cleaned}\"")
    } else {
        value.to_string()
    }
}

fn write_caldatetime(out: &mut String, prop_name: &str, dt: &CalDateTime) {
    match dt {
        CalDateTime::Utc(d) => {
            push_line(out, &format!("{prop_name}:{}", format_utc(d)));
        }
        CalDateTime::Floating(n) => {
            push_line(out, &format!("{prop_name}:{}", format_naive(n)));
        }
        CalDateTime::Zoned { tz_name, local } => {
            push_line(
                out,
                &format!("{prop_name};TZID={tz_name}:{}", format_naive(local)),
            );
        }
        CalDateTime::Date(d) => {
            push_line(out, &format!("{prop_name};VALUE=DATE:{}", format_date(d)));
        }
    }
}

fn write_vtimezone(out: &mut String, tz: &VTimezone) {
    push_line(out, "BEGIN:VTIMEZONE");
    push_line(out, &format!("TZID:{}", tz.tzid));
    for sub in &tz.raw_subs {
        write_raw_component(out, sub);
    }
    push_line(out, "END:VTIMEZONE");
}

fn write_raw_component(out: &mut String, c: &RawComponent) {
    push_line(out, &format!("BEGIN:{}", c.name));
    for p in &c.properties {
        push_line(out, &format_raw_property(p));
    }
    for child in &c.children {
        write_raw_component(out, child);
    }
    push_line(out, &format!("END:{}", c.name));
}

fn format_raw_property(p: &RawProperty) -> String {
    let mut s = p.name.clone();
    for (n, v) in &p.params {
        s.push_str(&format!(";{n}={}", quote_param_if_needed(v)));
    }
    s.push(':');
    s.push_str(&p.value);
    s
}

fn format_utc(d: &DateTime<Utc>) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute(),
        d.second()
    )
}

fn format_naive(n: &NaiveDateTime) -> String {
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        n.year(),
        n.month(),
        n.day(),
        n.hour(),
        n.minute(),
        n.second()
    )
}

fn format_date(d: &NaiveDate) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month(), d.day())
}

fn format_duration(d: &chrono::Duration) -> String {
    let total = d.num_seconds();
    let (sign, mut secs) = if total < 0 {
        ("-", -total)
    } else {
        ("", total)
    };
    let weeks = secs / (7 * 86400);
    if weeks > 0 && secs == weeks * 7 * 86400 {
        return format!("{sign}P{weeks}W");
    }
    let days = secs / 86400;
    secs -= days * 86400;
    let hours = secs / 3600;
    secs -= hours * 3600;
    let minutes = secs / 60;
    let secs = secs - minutes * 60;
    let mut s = format!("{sign}P");
    if days > 0 {
        s.push_str(&format!("{days}D"));
    }
    if hours > 0 || minutes > 0 || secs > 0 {
        s.push('T');
        if hours > 0 {
            s.push_str(&format!("{hours}H"));
        }
        if minutes > 0 {
            s.push_str(&format!("{minutes}M"));
        }
        if secs > 0 {
            s.push_str(&format!("{secs}S"));
        }
    }
    if s == "P" {
        s.push_str("T0S");
    }
    s
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            other => out.push(other),
        }
    }
    out
}

/// Append `line` to `out` with RFC 5545 §3.1 line folding (75 octets per
/// physical line) and CRLF terminator.
fn push_line(out: &mut String, line: &str) {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        out.push_str(line);
        out.push_str("\r\n");
        return;
    }
    // Fold: emit first 75 octets, then for each subsequent 74-octet chunk,
    // prefix with CRLF + SPACE. Octet boundaries respect UTF-8 char
    // boundaries by stepping back if needed.
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let chunk = if first { 75 } else { 74 };
        let mut end = std::cmp::min(start + chunk, bytes.len());
        // Step back to a char boundary.
        while end < bytes.len() && !line.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            out.push_str("\r\n ");
        }
        out.push_str(&line[start..end]);
        start = end;
        first = false;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod serialize_tests {
    use super::*;

    #[test]
    fn folds_long_line() {
        let mut out = String::new();
        // 200-char ASCII line.
        let long = "X".repeat(200);
        push_line(&mut out, &format!("DESCRIPTION:{long}"));
        // Each physical line, excluding CRLF and the leading SP, must be ≤ 75.
        for raw in out.split_terminator("\r\n") {
            let stripped = raw.strip_prefix(' ').unwrap_or(raw);
            assert!(stripped.len() <= 75, "physical line too long: {raw}");
        }
        assert!(out.ends_with("\r\n"));
    }

    #[test]
    fn short_line_not_folded() {
        let mut out = String::new();
        push_line(&mut out, "VERSION:2.0");
        assert_eq!(out, "VERSION:2.0\r\n");
    }

    #[test]
    fn escape_text_handles_specials() {
        assert_eq!(
            escape_text("hello, world; line1\nline2\\done"),
            "hello\\, world\\; line1\\nline2\\\\done"
        );
    }

    #[test]
    fn format_duration_round_numbers() {
        assert_eq!(format_duration(&chrono::Duration::seconds(3600)), "PT1H");
        assert_eq!(
            format_duration(&chrono::Duration::seconds(7 * 86400)),
            "P1W"
        );
        assert_eq!(
            format_duration(&chrono::Duration::seconds(90 * 60)),
            "PT1H30M"
        );
    }
}
