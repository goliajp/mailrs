//! A weekly meeting is on the calendar every week, at the hour it is that
//! week, and its card says it repeats and when it is next.
//!
//! The invitation is the shape of a Teams one: Thursdays at 22:00 in
//! `Pacific Standard Time`, with the organiser's `VTIMEZONE`. Filed as one
//! event, it used to come back once — on its first Thursday — so every
//! later week was empty and nothing after the first meeting conflicted.

mod live;

use mailrs_core_sidestate::families::calendar_events;
use reqwest::Method;

const KEVY: &str = "mem://webapi-weekly-meeting";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-sync@partner.example";
const MID: &str = "m-sync@partner.example";

const VTIMEZONE: &str = "BEGIN:VTIMEZONE\r\nTZID:Pacific Standard Time\r\n\
BEGIN:STANDARD\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0800\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\r\nEND:STANDARD\r\n\
BEGIN:DAYLIGHT\r\nDTSTART:16010101T020000\r\nTZOFFSETFROM:-0800\r\nTZOFFSETTO:-0700\r\n\
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n";

fn ics(method: &str, body: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:{method}\r\nPRODID:Microsoft Exchange Server 2010\r\n\
         {VTIMEZONE}BEGIN:VEVENT\r\nUID:golia-sync\r\nSEQUENCE:0\r\nDTSTAMP:20261028T010000Z\r\n\
         SUMMARY:GOLIA sync\r\nORGANIZER:mailto:chair@partner.example\r\n{body}END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// No end, so the card's next occurrence exists whenever this runs.
const SERIES: &str = "DTSTART;TZID=Pacific Standard Time:20261029T220000\r\n\
DTEND;TZID=Pacific Standard Time:20261029T230000\r\n\
RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=TH;WKST=SU\r\n";

/// The typed invite as ingest stores it beside the message.
fn payload(text: &str) -> serde_json::Value {
    let parsed = mailrs_ical::parse_invite(text.as_bytes()).expect("ics");
    let (start, end) = mailrs_ical::instants::instants_of(&parsed);
    let mut v = serde_json::to_value(&parsed).expect("json");
    let obj = v.as_object_mut().expect("object");
    obj.insert("dtstart_utc".into(), start.map(|t| t.to_rfc3339()).into());
    obj.insert("dtend_utc".into(), end.map(|t| t.to_rfc3339()).into());
    v
}

fn file(kv: &mut kevy_client::Connection, method: &str, invite: &serde_json::Value) {
    let row = calendar_events::from_invite("golia-sync", method, 0, invite);
    assert!(calendar_events::file_invitation(kv, ALICE, &row, invite).expect("file"));
}

/// A message with uid 9 carrying the series invitation.
fn deliver(live: &live::Live, invite: &serde_json::Value) {
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_793_000_000,
            latest_preview: "invitation",
            senders_csv: "chair@partner.example",
            subject: "GOLIA sync",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let row = serde_json::json!({
        "uid": 9, "sender": "chair@partner.example", "recipients": ALICE,
        "subject": "GOLIA sync", "date": 1_793_000_000, "internal_date": 1_793_000_000,
        "size": 900, "flags": 0, "message_id": MID, "in_reply_to": "",
        "thread_id": TID, "modseq": 1, "invite_method": "REQUEST",
    });
    live.mailbox
        .upsert_user_message(
            ALICE,
            TID,
            MID,
            1_793_000_000,
            row.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: "1793000000.M1P1.host",
                uid: 9,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("message row");
    live.mailbox.register_uid(ALICE, 9, MID).expect("uid");
    live.mailbox
        .store_ref()
        .hset(
            format!("mailrs:invite:{MID}").as_bytes(),
            &[
                (b"method".as_slice(), b"REQUEST".as_slice()),
                (b"uid".as_slice(), b"golia-sync".as_slice()),
                (b"sequence".as_slice(), b"0".as_slice()),
                (b"payload".as_slice(), invite.to_string().as_bytes()),
            ],
        )
        .expect("invite");
}

async fn get(live: &live::Live, path: &str, bearer: &str) -> serde_json::Value {
    let (status, body) = live.call(Method::GET, path, bearer, None).await;
    assert_eq!(status, 200, "{path}: {body}");
    body
}

fn starts(rows: &serde_json::Value) -> Vec<(String, String)> {
    rows.as_array()
        .expect("array")
        .iter()
        .map(|r| {
            assert_eq!(r["uid"], "golia-sync");
            (
                r["dtstart"].as_str().expect("start").to_string(),
                r["recurrence_id"]
                    .as_str()
                    .expect("recurrence_id")
                    .to_string(),
            )
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_week_is_on_the_calendar_and_the_card_says_when_it_is_next() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let series = payload(&ics("REQUEST", SERIES));
    deliver(&live, &series);
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    file(&mut kv, "REQUEST", &series);

    // three weeks across the US change on 1 November: 05:00 UTC, then 06:00
    let weeks = "/api/calendar/events?start=2026-10-26T00:00:00Z&end=2026-11-16T00:00:00Z";
    let at = |s: &str| (s.to_string(), s.to_string());
    assert_eq!(
        starts(&get(&live, weeks, &alice).await),
        [
            at("2026-10-30T05:00:00+00:00"),
            at("2026-11-06T06:00:00+00:00"),
            at("2026-11-13T06:00:00+00:00"),
        ]
    );
    // a week in January has its meeting too
    let january = "/api/calendar/events?start=2027-01-04T00:00:00Z&end=2027-01-11T00:00:00Z";
    assert_eq!(
        starts(&get(&live, january, &alice).await),
        [at("2027-01-08T06:00:00+00:00")]
    );
    // and it conflicts with a meeting on a later week, not only the first
    let conflicts = "/api/calendar/conflicts?start=2026-11-13T06:30:00Z&end=2026-11-13T07:00:00Z";
    assert_eq!(
        get(&live, conflicts, &alice).await.as_array().map(Vec::len),
        Some(1)
    );

    // the organiser cancels the 5 November meeting by a mail of its own
    let cancel = payload(&ics(
        "CANCEL",
        "RECURRENCE-ID;TZID=Pacific Standard Time:20261105T220000\r\n\
         DTSTART;TZID=Pacific Standard Time:20261105T220000\r\n\
         DTEND;TZID=Pacific Standard Time:20261105T230000\r\nSTATUS:CANCELLED\r\n",
    ));
    file(&mut kv, "CANCEL", &cancel);
    let rows = get(&live, weeks, &alice).await;
    assert_eq!(
        starts(&rows),
        [
            at("2026-10-30T05:00:00+00:00"),
            at("2026-11-13T06:00:00+00:00")
        ]
    );
    // one week cancelled is not the series cancelled
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] != "CANCELLED")
    );

    // the card: it repeats every Thursday, and the next one has not ended
    let detail = get(&live, "/api/mail/messages/9", &alice).await;
    let invite = &detail["invite_payload"];
    assert_eq!(invite["organiser_zone"], "America/Los_Angeles");
    assert_eq!(invite["recurrence"]["freq"], "WEEKLY");
    assert_eq!(invite["recurrence"]["by_day"], serde_json::json!(["TH"]));
    let next = &invite["next_occurrence"];
    let start = chrono::DateTime::parse_from_rfc3339(next["start"].as_str().expect("start"))
        .expect("an instant");
    let end = chrono::DateTime::parse_from_rfc3339(next["end"].as_str().expect("end")).unwrap();
    assert!(
        end > chrono::Utc::now(),
        "the next occurrence has not ended"
    );
    assert!(start < chrono::Utc::now() + chrono::Duration::days(7));
    let local = next["organiser_local"].as_str().expect("organiser's clock");
    assert!(local.ends_with("T22:00:00"), "{local}");
    let offset = next["organiser_offset_secs"].as_i64().expect("offset");
    assert!(offset == -7 * 3600 || offset == -8 * 3600, "{offset}");
    let wall = start.naive_utc() + chrono::Duration::seconds(offset);
    assert_eq!(wall.format("%Y-%m-%dT%H:%M:%S").to_string(), local);
}
