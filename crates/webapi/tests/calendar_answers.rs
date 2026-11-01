//! Answering an invitation puts the answer on the calendar, and the
//! calendar's events read back through the web: the reader's own, the
//! invitations, and each subscribed feed's, within the window asked for.
//! A declined meeting stays on the calendar and leaves the conflicts check.

mod live;

use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent};
use reqwest::Method;

const KEVY: &str = "mem://webapi-calendar-answers";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-invite@partner.example";
const MID: &str = "m-invite@partner.example";

const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n\
UID:board-1\r\nSEQUENCE:0\r\nDTSTAMP:20260801T000000Z\r\nDTSTART:20260901T090000Z\r\n\
DTEND:20260901T100000Z\r\nSUMMARY:Board meeting\r\nORGANIZER:mailto:chair@partner.example\r\n\
ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:alice@example.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// The typed invite as ingest stores it beside the message.
fn stored_invite() -> String {
    let parsed = mailrs_ical::parse_invite(ICS.as_bytes()).expect("ics");
    let (start, end) = mailrs_ical::instants::instants_of(&parsed);
    let mut v = serde_json::to_value(&parsed).expect("json");
    let obj = v.as_object_mut().expect("object");
    obj.insert("dtstart_utc".into(), start.map(|t| t.to_rfc3339()).into());
    obj.insert("dtend_utc".into(), end.map(|t| t.to_rfc3339()).into());
    v.to_string()
}

/// A message with uid 9 carrying the invitation — not yet on the calendar,
/// as for mail that arrived before invitations were filed.
fn deliver_invite(live: &live::Live) {
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_100,
            latest_preview: "invitation",
            senders_csv: "chair@partner.example",
            subject: "Board meeting",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let payload = serde_json::json!({
        "uid": 9, "sender": "chair@partner.example", "recipients": ALICE,
        "subject": "Board meeting", "date": 1_786_000_100, "internal_date": 1_786_000_100,
        "size": 900, "flags": 0, "message_id": MID, "in_reply_to": "",
        "thread_id": TID, "modseq": 1,
    });
    live.mailbox
        .upsert_user_message(
            ALICE,
            TID,
            MID,
            1_786_000_100,
            payload.to_string().as_bytes(),
            &mailrs_mailbox_kevy::UserMessageFacts {
                blob_ref: "1786000100.M1P1.host",
                uid: 9,
                flags: 0,
                modseq: 1,
            },
        )
        .expect("message row");
    live.mailbox.register_uid(ALICE, 9, MID).expect("uid");
    let invite = stored_invite();
    live.mailbox
        .store_ref()
        .hset(
            format!("mailrs:invite:{MID}").as_bytes(),
            &[
                (b"method".as_slice(), b"REQUEST".as_slice()),
                (b"uid".as_slice(), b"board-1".as_slice()),
                (b"sequence".as_slice(), b"0".as_slice()),
                (b"payload".as_slice(), invite.as_bytes()),
            ],
        )
        .expect("invite");
}

async fn get(live: &live::Live, path: &str, bearer: &str) -> serde_json::Value {
    let (status, body) = live.call(Method::GET, path, bearer, None).await;
    assert_eq!(status, 200, "{path}: {body}");
    body
}

async fn answer(live: &live::Live, bearer: &str, partstat: &str) {
    let (status, body) = live
        .call(
            Method::POST,
            "/api/invites/9/rsvp",
            bearer,
            Some(serde_json::json!({ "partstat": partstat })),
        )
        .await;
    // 202 when the reply could not be queued; the answer is recorded either way
    assert!(status == 200 || status == 202, "{status}: {body}");
}

fn uids(rows: &serde_json::Value) -> Vec<&str> {
    rows.as_array()
        .expect("array")
        .iter()
        .map(|r| r["uid"].as_str().expect("uid"))
        .collect()
}

const SEPTEMBER_FIRST: &str =
    "/api/calendar/events?start=2026-09-01T00:00:00Z&end=2026-09-02T00:00:00Z";
const CONFLICTS: &str =
    "/api/calendar/conflicts?start=2026-09-01T08:00:00Z&end=2026-09-01T12:00:00Z";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_is_on_the_calendar_and_a_decline_is_no_conflict() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    deliver_invite(&live);

    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    for (uid, start, source) in [
        ("standup", "2026-09-01T11:00:00+00:00", "feed:team"),
        ("next-week", "2026-09-08T11:00:00+00:00", "feed:team"),
    ] {
        let row = StoredEvent {
            uid: uid.into(),
            summary: uid.into(),
            dtstart: Some(start.into()),
            source: source.into(),
            ..Default::default()
        };
        calendar_events::put(&mut kv, ALICE, &row).expect("feed event");
    }
    assert_eq!(
        uids(&get(&live, SEPTEMBER_FIRST, &alice).await),
        ["standup"]
    );

    answer(&live, &alice, "ACCEPTED").await;
    let rows = get(&live, SEPTEMBER_FIRST, &alice).await;
    assert_eq!(uids(&rows), ["board-1", "standup"], "{rows}");
    assert_eq!(rows[0]["partstat"], "ACCEPTED", "{rows}");
    assert_eq!(rows[0]["summary"], "Board meeting", "{rows}");
    assert_eq!(rows[0]["source"], "mail:board-1", "{rows}");
    assert_eq!(rows[1]["source"], "feed:team", "{rows}");
    // the answer links the event to its mail, and the list says where it is
    assert_eq!(rows[0]["message_id"], MID, "{rows}");
    assert_eq!(rows[0]["thread_id"], TID, "{rows}");
    assert!(
        rows[1].get("thread_id").is_none(),
        "a feed event has no mail: {rows}"
    );
    assert!(
        rows[1].get("partstat").is_none(),
        "no answer is absent, not null: {rows}"
    );
    assert_eq!(
        uids(&get(&live, CONFLICTS, &alice).await),
        ["board-1", "standup"]
    );

    answer(&live, &alice, "DECLINED").await;
    let rows = get(&live, SEPTEMBER_FIRST, &alice).await;
    assert_eq!(rows[0]["partstat"], "DECLINED", "{rows}");
    assert_eq!(
        uids(&get(&live, CONFLICTS, &alice).await),
        ["standup"],
        "a declined meeting is not a conflict"
    );

    let (status, _) = live
        .call(
            Method::GET,
            "/api/calendar/events?start=soon&end=later",
            &alice,
            None,
        )
        .await;
    assert_eq!(status, 400);
}
