//! A calendar client may PUT an event under a resource name that is not
//! its `UID`. The row is still the UID's, so the web sees one event, the
//! invitation filed from mail for the same meeting is that same row, and
//! the client finds the event again at the href it chose. Through the
//! front door, against a real fastcore and network kevy.

mod live;

use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent};
use reqwest::Method;

const KEVY: &str = "mem://webapi-dav-resource-names";
const ALICE: &str = "alice@example.test";

fn event(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20260801T000000Z\r\n\
         DTSTART:20260901T090000Z\r\nDURATION:PT1H\r\nSUMMARY:Review\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

async fn dav(
    live: &live::Live,
    method: &str,
    path: &str,
    bearer: &str,
    body: &str,
) -> (u16, String) {
    let res = live
        .http
        .request(
            Method::from_bytes(method.as_bytes()).expect("method"),
            format!("{}{path}", live.base),
        )
        .bearer_auth(bearer)
        .header("depth", "1")
        .body(body.to_string())
        .send()
        .await
        .expect("request");
    let status = res.status().as_u16();
    (status, res.text().await.expect("body"))
}

async fn conflict_uids(live: &live::Live, bearer: &str) -> Vec<String> {
    let (status, rows) = live
        .call(
            Method::GET,
            "/api/calendar/conflicts?start=2026-09-01T08:00:00Z&end=2026-09-01T12:00:00Z",
            bearer,
            None,
        )
        .await;
    assert_eq!(status, 200);
    let rows = rows.as_array().expect("rows").clone();
    rows.iter()
        .map(|r| r["uid"].as_str().expect("uid").to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_event_is_its_uid_whatever_the_client_named_it() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let calendar = format!("/dav/calendars/{ALICE}/default/");
    let chosen = format!("{calendar}3F2504E0.ics");

    let (status, body) = dav(&live, "PUT", &chosen, &alice, &event("review@example.org")).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(conflict_uids(&live, &alice).await, ["review@example.org"]);

    // the same meeting filed from the emailed invitation, by the writer
    // fastcore's invite path uses
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let filed = StoredEvent {
        uid: "review@example.org".into(),
        summary: "Review".into(),
        dtstart: Some("2026-09-01T09:00:00+00:00".into()),
        source: "mail:review@example.org".into(),
        sequence: 1,
        ..Default::default()
    };
    calendar_events::put(&mut kv, ALICE, &filed).expect("file invite");
    assert_eq!(
        conflict_uids(&live, &alice).await,
        ["review@example.org"],
        "the invite and the client's copy are one row"
    );

    let query = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let (status, report) = dav(&live, "REPORT", &calendar, &alice, query).await;
    assert_eq!(status, 207);
    assert!(report.contains(&chosen), "{report}");
    assert!(!report.contains("review%40example.org.ics"), "{report}");
    assert!(!report.contains("review@example.org.ics"), "{report}");

    let (status, got) = dav(&live, "GET", &chosen, &alice, "").await;
    assert_eq!(status, 200);
    assert!(got.contains("UID:review@example.org"), "{got}");

    let other = format!("{calendar}copy.ics");
    let (status, refused) = dav(&live, "PUT", &other, &alice, &event("review@example.org")).await;
    assert_eq!(status, 403, "{refused}");
    assert!(refused.contains("<C:no-uid-conflict>"), "{refused}");
    assert!(
        refused.contains(&format!("<D:href>{chosen}</D:href>")),
        "{refused}"
    );
    assert_eq!(conflict_uids(&live, &alice).await, ["review@example.org"]);

    let (status, _) = dav(&live, "DELETE", &chosen, &alice, "").await;
    assert_eq!(status, 204);
    assert!(conflict_uids(&live, &alice).await.is_empty());
    let (status, _) = dav(&live, "GET", &chosen, &alice, "").await;
    assert_eq!(status, 404);
}
