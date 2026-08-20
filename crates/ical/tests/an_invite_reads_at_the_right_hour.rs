//! The hour an invitation is actually at, in the reader's zone.
//!
//! Every property of this fixture is copied from a real Teams invite
//! that arrived on 2026-08-19, with the people removed. Its `TZID` is
//! `Pacific Standard Time`: a **Windows** zone name, and one that says
//! "Standard" while the event is in daylight time. The embedded
//! `VTIMEZONE` carries what the organiser's server actually computed
//! against — daylight from the second Sunday in March, standard from
//! the first in November.
//!
//! 20 August falls between those, so the offset is −07:00 and
//! `20260820T160000` is 23:00 UTC — 08:00 the next morning in Tokyo.
//! Reading the *name* as "standard, therefore −08:00" gives 07:00:
//! wrong by an hour, in a way nobody would question, and only for half
//! the year.
//!
//! The other two tests are the hours that do not behave: the one that
//! does not exist on a spring-forward date, and the one that happens
//! twice on a fall-back date.

use chrono::NaiveDate;
use mailrs_ical::vtimezone::{ResolvedTz, local_to_utc_offset_seconds, resolve};
use mailrs_ical::{CalDateTime, parse_invite};

/// A Teams-shaped invitation: Windows TZID, inline VTIMEZONE, August.
const INVITE: &str = "\
BEGIN:VCALENDAR\r\n\
METHOD:REQUEST\r\n\
PRODID:Microsoft Exchange Server 2010\r\n\
VERSION:2.0\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\n\
DTSTART:16010101T020000\r\n\
TZOFFSETFROM:-0700\r\n\
TZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\n\
END:STANDARD\r\n\
BEGIN:DAYLIGHT\r\n\
DTSTART:16010101T020000\r\n\
TZOFFSETFROM:-0800\r\n\
TZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
ORGANIZER;CN=Chair:mailto:chair@example.com\r\n\
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:me@example.com\r\n\
UID:040000008200E00074C5B7101A82E00800000000EXAMPLE\r\n\
SUMMARY;LANGUAGE=en-US:Product sync\r\n\
DTSTART;TZID=Pacific Standard Time:20260820T160000\r\n\
DTEND;TZID=Pacific Standard Time:20260820T165000\r\n\
LOCATION;LANGUAGE=en-US:H-120 Teams Room\r\n\
DTSTAMP:20260819T152046Z\r\n\
SEQUENCE:9\r\n\
STATUS:CONFIRMED\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

/// The offset a wall-clock local time resolves to, through the invite's
/// own VTIMEZONE — the path a renderer takes.
fn offset_for(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> i32 {
    let parsed = parse_invite(INVITE.as_bytes()).expect("the fixture parses");
    let rt = resolve("Pacific Standard Time", &parsed.vtimezones)
        .expect("the inline VTIMEZONE resolves the Windows name");
    // Resolution came from the message, not from a table keyed on a
    // name that lies: the block wins over any external database, which
    // is what RFC 5545 §3.6.5 asks for.
    assert!(
        matches!(rt, ResolvedTz::Custom(_)),
        "the embedded block should have won"
    );
    let local = NaiveDate::from_ymd_opt(y, m, d)
        .unwrap()
        .and_hms_opt(hh, mm, 0)
        .unwrap();
    local_to_utc_offset_seconds(&rt, local).expect("an offset for a real local time")
}

#[test]
fn an_august_meeting_is_in_daylight_time_whatever_the_zone_is_called() {
    assert_eq!(
        offset_for(2026, 8, 20, 16, 0),
        -7 * 3600,
        "August is daylight time; reading the name \"Pacific Standard\" \
         as -08:00 puts the meeting an hour early"
    );

    // And the reader's side of it: 16:00 −07:00 is 23:00 UTC, which is
    // 08:00 the next morning in Tokyo — the number a person acts on.
    let parsed = parse_invite(INVITE.as_bytes()).unwrap();
    let CalDateTime::Zoned { local, tz_name } = &parsed.dtstart else {
        panic!("a zoned DTSTART, as the fixture writes it");
    };
    assert_eq!(tz_name.as_str(), "Pacific Standard Time");
    let utc = *local - chrono::Duration::seconds(offset_for(2026, 8, 20, 16, 0) as i64);
    assert_eq!(utc.format("%Y-%m-%dT%H:%M").to_string(), "2026-08-20T23:00");
    let tokyo = utc + chrono::Duration::hours(9);
    assert_eq!(
        tokyo.format("%Y-%m-%d %H:%M").to_string(),
        "2026-08-21 08:00",
        "this is the line a reader in Tokyo acts on"
    );
}

#[test]
fn the_same_meeting_in_january_is_standard_time() {
    assert_eq!(
        offset_for(2026, 1, 20, 16, 0),
        -8 * 3600,
        "January is standard time — the same zone, the other offset"
    );
}

/// 08 March 2026 is the second Sunday: 02:00 becomes 03:00 and 02:30
/// never happens. A resolver must still answer *something* rather than
/// panic — an invitation naming an impossible hour is a real thing
/// organisers send, and refusing to render it is worse than picking
/// the offset either side of the gap.
#[test]
fn the_hour_that_does_not_exist_still_resolves() {
    let off = offset_for(2026, 3, 8, 2, 30);
    assert!(
        off == -8 * 3600 || off == -7 * 3600,
        "expected one of the two offsets around the gap, got {off}"
    );
}

/// 01 November 2026 is the first Sunday: 01:30 happens twice. Whichever
/// is chosen, it must be one of the two — and it must be chosen the
/// same way every time, or a meeting moves an hour between renders.
#[test]
fn the_hour_that_happens_twice_resolves_the_same_way_each_time() {
    let first = offset_for(2026, 11, 1, 1, 30);
    let second = offset_for(2026, 11, 1, 1, 30);
    assert_eq!(first, second, "the ambiguous hour must not wobble");
    assert!(
        first == -7 * 3600 || first == -8 * 3600,
        "expected one of the two offsets around the repeat, got {first}"
    );
}

/// And the whole of it in one call — the shape the server stores and
/// the client reads, so nobody downstream has to know what a VTIMEZONE
/// is.
#[test]
fn the_invite_resolves_to_the_instant_a_reader_acts_on() {
    let parsed = parse_invite(INVITE.as_bytes()).unwrap();
    let (start, end) = mailrs_ical::instants::instants_of(&parsed);
    let start = start.expect("a zoned DTSTART resolves to an instant");
    let end = end.expect("and so does its DTEND");
    assert_eq!(
        start.format("%Y-%m-%dT%H:%MZ").to_string(),
        "2026-08-20T23:00Z"
    );
    assert_eq!(
        end.format("%Y-%m-%dT%H:%MZ").to_string(),
        "2026-08-20T23:50Z"
    );
}

/// An all-day event has no offset, and giving it one is how it lands on
/// the wrong day for a reader west of the organiser.
#[test]
fn an_all_day_event_is_left_alone() {
    let all_day = INVITE
        .replace(
            "DTSTART;TZID=Pacific Standard Time:20260820T160000",
            "DTSTART;VALUE=DATE:20260820",
        )
        .replace(
            "DTEND;TZID=Pacific Standard Time:20260820T165000",
            "DTEND;VALUE=DATE:20260821",
        );
    let parsed = parse_invite(all_day.as_bytes()).expect("the all-day fixture parses");
    let (start, end) = mailrs_ical::instants::instants_of(&parsed);
    assert!(
        start.is_none() && end.is_none(),
        "an all-day event must not be converted to an instant"
    );
    assert!(matches!(parsed.dtstart, CalDateTime::Date(_)));
}

/// A reply carries exactly one attendee — the person replying.
///
/// Sending the whole guest list back is the classic mistake: Exchange
/// reads it as one person answering for everybody, and either refuses
/// the reply or rewrites the other guests' states. It also has to echo
/// the UID and the SEQUENCE, or the organiser cannot tell which
/// revision is being answered.
#[test]
fn a_reply_answers_for_one_person_and_names_the_revision() {
    use mailrs_ical::{Method, PartStat, reply};

    let mut request = parse_invite(INVITE.as_bytes()).unwrap();
    // A second guest, so "exactly one" is a real assertion rather than
    // a property of a one-guest fixture.
    request.attendees.push(mailrs_ical::Attendee {
        cn: None,
        email: "other@example.com".into(),
        partstat: PartStat::NeedsAction,
        role: mailrs_ical::Role::ReqParticipant,
        rsvp: true,
    });

    let built = reply::build(&request, "ME@example.com", PartStat::Accepted)
        .expect("the address is on the guest list, whatever its case");
    assert_eq!(built.method, Method::Reply);
    assert_eq!(built.uid, request.uid);
    assert_eq!(built.sequence, 9, "the reply names the revision it answers");
    assert_eq!(built.attendees.len(), 1, "a reply speaks for one person");
    assert_eq!(built.attendees[0].partstat, PartStat::Accepted);
    assert!(!built.attendees[0].rsvp, "an answer needs no answer");

    let body = reply::serialize_reply(&request, "me@example.com", PartStat::Declined)
        .expect("it serialises");
    assert!(body.contains("METHOD:REPLY"));
    assert!(body.contains("PARTSTAT=DECLINED"));
    assert_eq!(
        body.matches("ATTENDEE").count(),
        1,
        "exactly one ATTENDEE line, or Exchange rewrites the other guests"
    );
    assert!(
        body.contains(&format!("UID:{}", request.uid)),
        "the organiser matches the reply to the request by UID"
    );
}

/// Replying as somebody the organiser never invited is not a reply.
#[test]
fn a_stranger_cannot_reply() {
    use mailrs_ical::{PartStat, reply};
    let request = parse_invite(INVITE.as_bytes()).unwrap();
    assert!(reply::build(&request, "nobody@example.com", PartStat::Accepted).is_none());
}

/// The reply as a message, and the two things an organiser's server
/// checks before it will believe one.
#[test]
fn the_reply_message_is_addressed_to_the_organiser_and_carries_the_method() {
    use mailrs_ical::{PartStat, mime_part::extract_invite_part, reply};

    let request = parse_invite(INVITE.as_bytes()).unwrap();
    let msg = reply::reply_message(
        &request,
        "me@example.com",
        PartStat::Accepted,
        "Thu, 20 Aug 2026 12:00:00 +0900",
        "<r1@golia.jp>",
    )
    .expect("it builds");
    let text = String::from_utf8(msg.clone()).unwrap();

    assert!(
        text.contains("To: chair@example.com"),
        "replies go to the organiser"
    );
    assert!(text.contains("Subject: Accepted: Product sync"));
    assert!(
        text.contains("method=REPLY"),
        "some clients route on the Content-Type parameter alone"
    );

    // And it survives the trip back through our own extractor — the
    // same code the receiving side would use.
    let back = extract_invite_part(&msg).expect("the calendar part is findable");
    let parsed = parse_invite(&back.ics_bytes).expect("and parses");
    assert_eq!(parsed.method, mailrs_ical::Method::Reply);
    assert_eq!(parsed.uid, request.uid);
    assert_eq!(parsed.attendees.len(), 1);
}
