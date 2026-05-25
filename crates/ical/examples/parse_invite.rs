//! End-to-end example: parse an iCalendar REQUEST, print the typed view,
//! then round-trip it through the serializer.
//!
//! Run with: `cargo run -p mailrs-ical --example parse_invite`

use mailrs_ical::{CalDateTime, Method, parse_invite, serialize};

fn main() {
    let ics = b"BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Example Corp.//Cal//EN\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:9ad12c8e-4f3b-11ee-be56-0242ac120002\r\n\
SEQUENCE:0\r\n\
DTSTAMP:20260120T130000Z\r\n\
DTSTART:20260205T100000Z\r\n\
DTEND:20260205T110000Z\r\n\
ORGANIZER;CN=Alice:mailto:alice@example.org\r\n\
ATTENDEE;CN=Bob;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:bob@example.com\r\n\
SUMMARY:Quarterly sync\r\n\
LOCATION:Room 101\r\n\
DESCRIPTION:Status update + planning.\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    let invite = parse_invite(ics).expect("parse failed");

    println!("--- parsed ---");
    println!("  method:    {:?}", invite.method);
    println!("  uid:       {}", invite.uid);
    println!("  sequence:  {}", invite.sequence);
    println!("  dtstamp:   {}", invite.dtstamp);
    match &invite.dtstart {
        CalDateTime::Utc(dt) => println!("  dtstart:   {dt}"),
        CalDateTime::Floating(dt) => println!("  dtstart:   {dt} (floating)"),
        CalDateTime::Zoned { tz_name, local } => println!("  dtstart:   {local} {tz_name}"),
        CalDateTime::Date(d) => println!("  dtstart:   {d} (all-day)"),
    }
    println!("  organizer: {:?}", invite.organizer);
    println!("  attendees:");
    for a in &invite.attendees {
        println!(
            "    - {} <{}>  role={:?} partstat={:?}",
            a.cn.as_deref().unwrap_or("?"),
            a.email,
            a.role,
            a.partstat
        );
    }
    println!("  summary:   {}", invite.summary);
    println!("  location:  {:?}", invite.location);

    assert_eq!(invite.method, Method::Request);

    // build a REPLY by flipping METHOD + PARTSTAT
    let mut reply = invite.clone();
    reply.method = Method::Reply;
    for a in reply.attendees.iter_mut() {
        if a.email == "bob@example.com" {
            a.partstat = mailrs_ical::PartStat::Accepted;
        }
    }
    let reply_text = serialize::serialize(&reply).expect("serialize failed");

    println!("\n--- REPLY serialized ---");
    println!("{reply_text}");
}
