//! An all-day invitation in the mail is filed on the calendar by its
//! dates. Until this, its start had no instant, the event was stored
//! without one, and every calendar listing left it out.

use mailrs_core_sidestate::families::calendar_events as ev;

const MAIL: &str = "\
From: chair@example.com\r\n\
To: a@example.test\r\n\
Subject: Offsite\r\n\
MIME-Version: 1.0\r\n\
Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
\r\n\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:offsite-2026\r\n\
SUMMARY:Offsite\r\n\
DTSTAMP:20261101T000000Z\r\n\
DTSTART;VALUE=DATE:20261103\r\n\
DTEND;VALUE=DATE:20261105\r\n\
ORGANIZER:mailto:chair@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

#[test]
fn an_all_day_invitation_is_listed_on_its_days() {
    let found = mailrs_intake::invite::find(MAIL.as_bytes()).expect("an invite");
    // held open across the filing: a mem:// store lives while a connection does
    let mut reader = kevy_client::Connection::connect("mem://all-day-invitation").expect("kevy");
    let conn = kevy_client::Connection::connect("mem://all-day-invitation").expect("kevy");
    mailrs_intake::invite::file_event(Some(conn), "a@example.test", &found, "m1@example.com");

    let listed = ev::list(&mut reader, "a@example.test").expect("list");
    let got: Vec<_> = listed
        .iter()
        .map(|e| (e.uid.as_str(), e.dtstart.as_deref(), e.dtend.as_deref()))
        .collect();
    assert_eq!(
        got,
        [("offsite-2026", Some("2026-11-03"), Some("2026-11-05"))]
    );
}
