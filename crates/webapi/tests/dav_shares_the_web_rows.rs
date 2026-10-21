//! CalDAV and CardDAV read and write the rows the web reads: an event a
//! phone saves is in the conflicts check, an event filed from mail is in
//! the phone's calendar, a card a phone saves is offered by autocomplete.
//! Through the front door, against a real fastcore and network kevy.

mod live;

use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent};
use reqwest::Method;

const KEVY: &str = "mem://webapi-dav-shares-the-web-rows";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

fn event(uid: &str, start: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20260801T000000Z\r\n\
         DTSTART:{start}\r\nDURATION:PT1H\r\nSUMMARY:Dentist\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Lovelace\r\n\
    EMAIL:ada@engine.example\r\nEND:VCARD\r\n";

struct Reply {
    status: u16,
    etag: Option<String>,
    body: String,
}

async fn dav(
    live: &live::Live,
    method: &str,
    path: &str,
    bearer: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Reply {
    let method = Method::from_bytes(method.as_bytes()).expect("method");
    let mut req = live
        .http
        .request(method, format!("{}{path}", live.base))
        .bearer_auth(bearer)
        .body(body.to_string());
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = req.send().await.expect("request");
    let status = res.status().as_u16();
    let etag = res
        .headers()
        .get("etag")
        .map(|v| v.to_str().expect("ascii").to_string());
    Reply {
        status,
        etag,
        body: res.text().await.expect("body"),
    }
}

async fn conflicts(live: &live::Live, bearer: &str) -> serde_json::Value {
    let (status, rows) = live
        .call(
            Method::GET,
            "/api/calendar/conflicts?start=2026-09-01T08:00:00Z&end=2026-09-01T12:00:00Z",
            bearer,
            None,
        )
        .await;
    assert_eq!(status, 200);
    rows
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dav_reads_and_writes_the_rows_the_web_reads() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    caldav_put_is_seen_by_the_conflicts_check(&live, &alice, &bob).await;
    an_event_filed_from_mail_is_in_the_caldav_report(&live, &alice).await;
    carddav_put_is_offered_by_autocomplete(&live, &alice).await;
    etag_preconditions_guard_a_put(&live, &alice).await;
    one_user_cannot_reach_another(&live, &bob).await;
    a_bad_body_or_a_failed_store_is_never_a_201(&live, &alice).await;
    the_migration_moves_old_keys_once(&live, &alice).await;
}

async fn caldav_put_is_seen_by_the_conflicts_check(live: &live::Live, alice: &str, bob: &str) {
    let path = format!("/dav/calendars/{ALICE}/default/phone-1.ics");
    let r = dav(
        live,
        "PUT",
        &path,
        alice,
        &[],
        &event("phone-1", "20260901T090000Z"),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.body);
    assert!(r.etag.is_some());

    let rows = conflicts(live, alice).await;
    assert!(rows.to_string().contains("phone-1"), "alice: {rows}");
    let rows = conflicts(live, bob).await;
    assert_eq!(rows, serde_json::json!([]), "bob sees alice's event");

    let got = dav(live, "GET", &path, alice, &[], "").await;
    assert_eq!(
        got.body,
        event("phone-1", "20260901T090000Z"),
        "the text round-trips"
    );
}

async fn an_event_filed_from_mail_is_in_the_caldav_report(live: &live::Live, alice: &str) {
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let filed = StoredEvent {
        uid: "invite-1".into(),
        summary: "Board meeting".into(),
        dtstart: Some("2026-09-02T09:00:00+00:00".into()),
        source: "mail:invite-1".into(),
        ..Default::default()
    };
    calendar_events::put(&mut kv, ALICE, &filed).expect("file");

    let query = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let path = format!("/dav/calendars/{ALICE}/default/");
    let r = dav(live, "REPORT", &path, alice, &[("depth", "1")], query).await;
    assert_eq!(r.status, 207);
    assert!(r.body.contains("invite-1.ics"), "{}", r.body);
    assert!(r.body.contains("SUMMARY:Board meeting"), "{}", r.body);
    assert!(r.body.contains("phone-1.ics"), "{}", r.body);

    let home = format!("/dav/calendars/{ALICE}/");
    let r = dav(live, "PROPFIND", &home, alice, &[("depth", "1")], "").await;
    assert_eq!(r.status, 207);
    assert!(r.body.contains(&format!("/dav/calendars/{ALICE}/default/")));
}

async fn carddav_put_is_offered_by_autocomplete(live: &live::Live, alice: &str) {
    let path = format!("/dav/addressbooks/{ALICE}/default/ada.vcf");
    let r = dav(live, "PUT", &path, alice, &[], CARD).await;
    assert_eq!(r.status, 201, "{}", r.body);

    let (status, offered) = live
        .call(Method::GET, "/api/contacts?q=ada", alice, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        offered,
        serde_json::json!(["Ada Lovelace <ada@engine.example>"])
    );

    let book = format!("/dav/addressbooks/{ALICE}/default/");
    let r = dav(live, "PROPFIND", &book, alice, &[("depth", "1")], "").await;
    assert!(r.body.contains("ada.vcf"), "{}", r.body);
}

async fn etag_preconditions_guard_a_put(live: &live::Live, alice: &str) {
    let path = format!("/dav/calendars/{ALICE}/default/phone-1.ics");
    let current = dav(live, "GET", &path, alice, &[], "")
        .await
        .etag
        .expect("etag");
    let next = event("phone-1", "20260901T100000Z");

    let stale = dav(
        live,
        "PUT",
        &path,
        alice,
        &[("if-match", "\"0000\"")],
        &next,
    )
    .await;
    assert_eq!(stale.status, 412);
    let exists = dav(live, "PUT", &path, alice, &[("if-none-match", "*")], &next).await;
    assert_eq!(exists.status, 412);
    let ok = dav(live, "PUT", &path, alice, &[("if-match", &current)], &next).await;
    assert_eq!(ok.status, 204);
    assert_ne!(ok.etag.as_deref(), Some(current.as_str()));
}

async fn one_user_cannot_reach_another(live: &live::Live, bob: &str) {
    let theirs = format!("/dav/calendars/{ALICE}/default/phone-1.ics");
    assert_eq!(dav(live, "GET", &theirs, bob, &[], "").await.status, 403);
    let put = dav(
        live,
        "PUT",
        &theirs,
        bob,
        &[],
        &event("phone-1", "20260901T090000Z"),
    )
    .await;
    assert_eq!(put.status, 403);
    let card = format!("/dav/addressbooks/{ALICE}/default/ada.vcf");
    assert_eq!(dav(live, "DELETE", &card, bob, &[], "").await.status, 403);

    let own = format!("/dav/calendars/{BOB}/default/phone-1.ics");
    assert_eq!(dav(live, "GET", &own, bob, &[], "").await.status, 404);
    let own_book = format!("/dav/addressbooks/{BOB}/default/");
    let r = dav(live, "PROPFIND", &own_book, bob, &[("depth", "1")], "").await;
    assert!(!r.body.contains("ada.vcf"), "{}", r.body);
}

async fn a_bad_body_or_a_failed_store_is_never_a_201(live: &live::Live, alice: &str) {
    let path = format!("/dav/calendars/{ALICE}/default/junk.ics");
    let r = dav(live, "PUT", &path, alice, &[], "not a calendar").await;
    assert_eq!(r.status, 400);

    // a subscription row nothing can parse: listing calendars must fail
    // loudly rather than serve a list with a calendar missing
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let feeds = mailrs_core_sidestate::families::calendar_feeds::feeds_key(ALICE);
    kv.hset(feeds.as_bytes(), &[(b"f1".as_slice(), b"{".as_slice())])
        .expect("seed");
    let home = format!("/dav/calendars/{ALICE}/");
    let r = dav(live, "PROPFIND", &home, alice, &[("depth", "1")], "").await;
    assert_eq!(r.status, 500);
    let r = dav(
        live,
        "PUT",
        &path,
        alice,
        &[],
        &event("junk", "20260901T090000Z"),
    )
    .await;
    assert_eq!(r.status, 500, "a write that could not resolve its calendar");
    kv.del(&[feeds.as_bytes()]).expect("clean");
}

async fn the_migration_moves_old_keys_once(live: &live::Live, alice: &str) {
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    let old = event("legacy-1", "20260901T110000Z");
    kv.hset(
        format!("caldav:{ALICE}:events:default").as_bytes(),
        &[(b"legacy-1".as_slice(), old.as_bytes())],
    )
    .expect("seed");
    let url = format!("{}/v1/admin/maintenance:migrate-dav-keys", live.core_url);
    let run = || async {
        let res = live.http.post(&url).send().await.expect("post");
        assert_eq!(res.status(), 200);
        res.json::<serde_json::Value>().await.expect("json")
    };
    let first = run().await;
    assert_eq!(first["events"]["walked"], 1, "{first}");
    assert_eq!(first["events"]["written"], 1, "{first}");
    let second = run().await;
    assert_eq!(second["events"]["walked"], 1, "{second}");
    assert_eq!(second["events"]["written"], 0, "{second}");
    assert!(
        conflicts(live, alice)
            .await
            .to_string()
            .contains("legacy-1")
    );
}
