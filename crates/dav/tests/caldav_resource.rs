//! Integration tests for CalDAV resource endpoints: GET / PUT / DELETE on
//! `/dav/calendars/{user}/{calendar}/{uid}.ics`.
//!
//! PRIORITY surface — every documented HTTP status, every precondition path,
//! end-to-end PUT → GET round-trip.

mod common;

use common::{
    InMemoryCalendarStore, body_as_str, header_value, make_calendar, make_event,
};
use mailrs_dav::caldav::{event_delete, event_get, event_put};
use mailrs_dav::error::DavError;
use mailrs_dav::xml::etag_of;

const CAL_ID: i64 = 10;
const ICAL_V1: &str = "BEGIN:VEVENT\nUID:evt-1\nSUMMARY:original\nEND:VEVENT";
const ICAL_V2: &str = "BEGIN:VEVENT\nUID:evt-1\nSUMMARY:updated\nEND:VEVENT";

fn fixture_with_event(uid: &str, body: &str) -> InMemoryCalendarStore {
    InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"))
        .with_event(CAL_ID, make_event(uid, body))
}

// ---------- GET ----------

#[tokio::test]
async fn event_get_returns_200_with_icalendar_body_and_quoted_etag() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let resp = event_get(&store, CAL_ID, "evt-1").await.unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(
        header_value(&resp.headers, "content-type"),
        Some("text/calendar; charset=utf-8")
    );
    let etag_header = header_value(&resp.headers, "etag").unwrap();
    assert!(etag_header.starts_with('"') && etag_header.ends_with('"'));
    assert_eq!(body_as_str(resp.body), ICAL_V1);
}

#[tokio::test]
async fn event_get_missing_uid_returns_not_found_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let err = event_get(&store, CAL_ID, "missing").await.unwrap_err();

    assert!(matches!(err, DavError::NotFound));
}

#[tokio::test]
async fn event_get_etag_header_matches_stored_etag() {
    let store = fixture_with_event("evt-1", ICAL_V1);
    let expected = etag_of(ICAL_V1);

    let resp = event_get(&store, CAL_ID, "evt-1").await.unwrap();

    assert_eq!(
        header_value(&resp.headers, "etag"),
        Some(format!("\"{expected}\"").as_str())
    );
}

// ---------- PUT ----------

#[tokio::test]
async fn event_put_new_returns_201_with_etag() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let resp = event_put(&store, CAL_ID, "evt-1", None, None, ICAL_V1)
        .await
        .unwrap();

    assert_eq!(resp.status, 201);
    let expected = etag_of(ICAL_V1);
    assert_eq!(
        header_value(&resp.headers, "etag"),
        Some(format!("\"{expected}\"").as_str())
    );
}

#[tokio::test]
async fn event_put_existing_returns_204_with_etag() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let resp = event_put(&store, CAL_ID, "evt-1", None, None, ICAL_V2)
        .await
        .unwrap();

    assert_eq!(resp.status, 204);
    let expected_new = etag_of(ICAL_V2);
    assert_eq!(
        header_value(&resp.headers, "etag"),
        Some(format!("\"{expected_new}\"").as_str())
    );
}

#[tokio::test]
async fn event_put_then_get_round_trips_body_unchanged() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let body =
        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:evt-1\r\nSUMMARY:hello\r\nEND:VEVENT\r\nEND:VCALENDAR";

    event_put(&store, CAL_ID, "evt-1", None, None, body)
        .await
        .unwrap();
    let resp = event_get(&store, CAL_ID, "evt-1").await.unwrap();

    assert_eq!(body_as_str(resp.body), body, "PUT body round-trips through GET unchanged");
}

#[tokio::test]
async fn event_put_if_match_with_correct_etag_succeeds() {
    let store = fixture_with_event("evt-1", ICAL_V1);
    let current_etag = etag_of(ICAL_V1);

    let resp = event_put(
        &store,
        CAL_ID,
        "evt-1",
        Some(&format!("\"{current_etag}\"")),
        None,
        ICAL_V2,
    )
    .await
    .unwrap();

    assert_eq!(resp.status, 204);
}

#[tokio::test]
async fn event_put_if_match_with_stale_etag_returns_precondition_failed() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let err = event_put(
        &store,
        CAL_ID,
        "evt-1",
        Some("\"deadbeefdeadbeef\""),
        None,
        ICAL_V2,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    // store must NOT have been mutated
    let evt = &store.events_in(CAL_ID)[0];
    assert_eq!(evt.icalendar, ICAL_V1);
}

#[tokio::test]
async fn event_put_if_match_strips_surrounding_quotes_before_comparison() {
    let store = fixture_with_event("evt-1", ICAL_V1);
    let current_etag = etag_of(ICAL_V1);

    // Some clients send the etag with quotes, others without — handler must
    // accept both. The current implementation strips surrounding quotes.
    let resp_quoted = event_put(
        &store,
        CAL_ID,
        "evt-1",
        Some(&format!("\"{current_etag}\"")),
        None,
        ICAL_V1,
    )
    .await
    .unwrap();
    assert_eq!(resp_quoted.status, 204);

    let resp_unquoted = event_put(&store, CAL_ID, "evt-1", Some(&current_etag), None, ICAL_V1)
        .await
        .unwrap();
    assert_eq!(resp_unquoted.status, 204);
}

#[tokio::test]
async fn event_put_if_match_on_missing_resource_returns_precondition_failed() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let err = event_put(
        &store,
        CAL_ID,
        "evt-missing",
        Some("\"any-etag\""),
        None,
        ICAL_V1,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    assert!(store.events_in(CAL_ID).is_empty());
}

#[tokio::test]
async fn event_put_if_none_match_star_blocks_overwrite_of_existing() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let err = event_put(&store, CAL_ID, "evt-1", None, Some("*"), ICAL_V2)
        .await
        .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    // body stays at v1
    let evt = &store.events_in(CAL_ID)[0];
    assert_eq!(evt.icalendar, ICAL_V1);
}

#[tokio::test]
async fn event_put_if_none_match_star_allows_create_when_resource_absent() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let resp = event_put(&store, CAL_ID, "evt-new", None, Some("*"), ICAL_V1)
        .await
        .unwrap();

    assert_eq!(resp.status, 201);
    assert_eq!(store.events_in(CAL_ID).len(), 1);
}

// ---------- DELETE ----------

#[tokio::test]
async fn event_delete_existing_returns_204_and_removes_row() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let resp = event_delete(&store, CAL_ID, "evt-1").await.unwrap();

    assert_eq!(resp.status, 204);
    assert!(store.events_in(CAL_ID).is_empty());
}

#[tokio::test]
async fn event_delete_missing_returns_not_found_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(common::TEST_USER, make_calendar(CAL_ID, "Work"));

    let err = event_delete(&store, CAL_ID, "missing").await.unwrap_err();

    assert!(matches!(err, DavError::NotFound));
}

#[tokio::test]
async fn event_delete_twice_second_call_returns_not_found() {
    let store = fixture_with_event("evt-1", ICAL_V1);

    let first = event_delete(&store, CAL_ID, "evt-1").await.unwrap();
    assert_eq!(first.status, 204);
    let second = event_delete(&store, CAL_ID, "evt-1").await.unwrap_err();
    assert!(matches!(second, DavError::NotFound));
}
