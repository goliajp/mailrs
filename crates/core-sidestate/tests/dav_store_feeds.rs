//! A subscribed feed shows over CalDAV as its own calendar, which only the
//! feed sync writes.

use std::sync::Mutex;

use kevy_client::Connection;
use mailrs_core_sidestate::families::calendar_events::{self, StoredEvent};
use mailrs_core_sidestate::families::calendar_feeds::{FeedRow, feeds_key};
use mailrs_core_sidestate::families::dav_store::{DavStore, SingleConn};
use mailrs_dav::DavError;
use mailrs_dav::dispatch::{DavRequest, handle};

const KEVY: &str = "mem://dav-store-feeds";
const USER: &str = "alice@example.test";

fn feed() -> FeedRow {
    serde_json::from_value(serde_json::json!({
        "id": "f1", "name": "Team", "url": "https://example.test/team.ics"
    }))
    .expect("feed row")
}

fn req<'a>(method: &'a str, path: &'a str, body: &'a str) -> DavRequest<'a> {
    DavRequest {
        method,
        path,
        depth: Some("1"),
        if_match: None,
        if_none_match: None,
        body,
    }
}

#[tokio::test]
async fn a_feed_is_a_read_only_calendar_of_its_own() {
    let mut kv = Connection::connect(KEVY).expect("kevy");
    let json = serde_json::to_vec(&feed()).expect("json");
    kv.hset(
        feeds_key(USER).as_bytes(),
        &[(b"f1".as_slice(), json.as_slice())],
    )
    .expect("seed feed");
    let synced = StoredEvent {
        uid: "standup".into(),
        summary: "Standup".into(),
        dtstart: Some("2026-09-01T09:00:00+00:00".into()),
        source: "feed:f1".into(),
        ..Default::default()
    };
    calendar_events::put(&mut kv, USER, &synced).expect("seed event");

    let store = DavStore::new(
        USER,
        SingleConn(Mutex::new(Connection::connect(KEVY).expect("kevy"))),
    );
    let home = format!("/dav/calendars/{USER}/");
    let r = handle(&req("PROPFIND", &home, ""), USER, &store, &store)
        .await
        .expect("home");
    let body = String::from_utf8(r.body).expect("utf8");
    assert!(
        body.contains(&format!("/dav/calendars/{USER}/Team/")),
        "{body}"
    );

    let team = format!("/dav/calendars/{USER}/Team/");
    let r = handle(&req("PROPFIND", &team, ""), USER, &store, &store)
        .await
        .expect("feed calendar");
    let body = String::from_utf8(r.body).expect("utf8");
    assert!(body.contains("standup.ics"), "{body}");

    let default = format!("/dav/calendars/{USER}/default/");
    let r = handle(&req("PROPFIND", &default, ""), USER, &store, &store)
        .await
        .expect("default calendar");
    let body = String::from_utf8(r.body).expect("utf8");
    assert!(
        !body.contains("standup.ics"),
        "feed events stay in their calendar: {body}"
    );

    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:standup\r\nDTSTAMP:20260801T000000Z\r\n\
               DTSTART:20260901T090000Z\r\nSUMMARY:Mine now\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let into_feed = format!("{team}standup.ics");
    let err = handle(&req("PUT", &into_feed, ics), USER, &store, &store).await;
    assert_eq!(err.err(), Some(DavError::Forbidden));
    let over_feed_uid = format!("{default}standup.ics");
    let err = handle(&req("PUT", &over_feed_uid, ics), USER, &store, &store).await;
    assert_eq!(err.err(), Some(DavError::Forbidden));
    let err = handle(&req("DELETE", &into_feed, ""), USER, &store, &store).await;
    assert_eq!(err.err(), Some(DavError::Forbidden));

    let kept = calendar_events::get(&mut kv, USER, "standup")
        .expect("get")
        .expect("row");
    assert_eq!(kept.summary, "Standup");
}
