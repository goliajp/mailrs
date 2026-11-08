//! Delivery records an invitation as one, for the sectioned inbox: it is
//! answered with the card's buttons, so it does not wait in Reply.

struct NoThreads;

impl mailrs_intake::arrival::Threads for NoThreads {
    fn thread_for_message_id(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn thread_subject(&self, _: &str, _: &str) -> Option<String> {
        None
    }
}

const ME: &str = "a@example.test";

fn mail(content_type: &str, body: &str) -> String {
    format!(
        "From: chair@example.com\r\n\
         To: {ME}\r\n\
         Subject: Offsite\r\n\
         Message-ID: <offsite@example.com>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: {content_type}\r\n\
         \r\n\
         {body}"
    )
}

const EVENT: &str = "\
BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
METHOD:REQUEST\r\n\
BEGIN:VEVENT\r\n\
UID:offsite-2026\r\n\
SUMMARY:Offsite\r\n\
DTSTAMP:20261101T000000Z\r\n\
DTSTART:20261103T010000Z\r\n\
DTEND:20261103T020000Z\r\n\
ORGANIZER:mailto:chair@example.com\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

fn derive(raw: &str) -> mailrs_intake::arrival::Arrival<'_> {
    mailrs_intake::arrival::derive(
        &NoThreads,
        None,
        ME,
        "1.host",
        raw.as_bytes(),
        "INBOX",
        None,
    )
}

#[test]
fn an_invitation_is_recorded_as_one() {
    let raw = mail("text/calendar; method=REQUEST; charset=utf-8", EVENT);
    let a = derive(&raw);
    assert!(a.facts.invite, "an invitation was not recorded as one");
    assert_eq!(
        a.invite.as_ref().map(|i| i.method.as_str()),
        Some("REQUEST")
    );

    let plain = mail("text/plain; charset=utf-8", "See you there.\r\n");
    let p = derive(&plain);
    assert!(!p.facts.invite && p.invite.is_none());
}
