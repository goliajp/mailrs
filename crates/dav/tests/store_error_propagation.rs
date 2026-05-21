//! Integration tests that systematically exercise every
//! `.map_err(|e| DavError::ServerError(...))` branch in the handlers.
//!
//! For each fallible store method, inject an error in the fixture, drive the
//! relevant handler, and assert the error surfaces as `DavError::ServerError`
//! carrying the description verbatim.


use mailrs_dav::fixtures::{
    InMemoryAddressBookStore, InMemoryCalendarStore, EXAMPLE_USER, make_book, make_calendar,
    make_contact, make_event,
};
use mailrs_dav::caldav::{
    calendar_home_propfind, calendar_propfind, calendar_report, event_delete, event_get, event_put,
};
use mailrs_dav::carddav::{
    addressbook_home_propfind, addressbook_propfind, addressbook_report, contact_delete,
    contact_get, contact_put,
};
use mailrs_dav::error::DavError;

fn expect_server_error(err: DavError, expected_msg: &str) {
    match err {
        DavError::ServerError(msg) => {
            assert!(
                msg.contains(expected_msg),
                "expected ServerError containing {expected_msg:?}, got {msg:?}"
            );
        }
        other => panic!("expected ServerError, got {other:?}"),
    }
}

// ---------- calendar surface ----------

#[tokio::test]
async fn calendar_home_propfind_ensure_default_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new().ensure_default_fails("default-bootstrap boom");

    let err = calendar_home_propfind(&store, EXAMPLE_USER, 0).await.unwrap_err();

    expect_server_error(err, "default-bootstrap boom");
}

#[tokio::test]
async fn calendar_home_propfind_list_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(1, "Work"))
        .list_calendars_fails("calendars unavailable");

    let err = calendar_home_propfind(&store, EXAMPLE_USER, 1).await.unwrap_err();

    expect_server_error(err, "calendars unavailable");
}

#[tokio::test]
async fn calendar_propfind_list_events_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .list_events_fails("event index missing");

    let err = calendar_propfind(&store, EXAMPLE_USER, "Work", 10, 1)
        .await
        .unwrap_err();

    expect_server_error(err, "event index missing");
}

#[tokio::test]
async fn calendar_report_list_events_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .list_events_fails("report query failed");

    let body = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let err = calendar_report(&store, EXAMPLE_USER, "Work", 10, body)
        .await
        .unwrap_err();

    expect_server_error(err, "report query failed");
}

#[tokio::test]
async fn event_get_store_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .get_event_fails("get borked");

    let err = event_get(&store, 10, "evt-1").await.unwrap_err();

    expect_server_error(err, "get borked");
}

#[tokio::test]
async fn event_put_etag_lookup_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .event_etag_fails("etag lookup failed");

    let err = event_put(
        &store,
        10,
        "evt-1",
        Some("\"deadbeef\""),
        None,
        "BEGIN:VEVENT\nEND:VEVENT",
    )
    .await
    .unwrap_err();

    expect_server_error(err, "etag lookup failed");
}

#[tokio::test]
async fn event_put_write_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .put_event_fails("disk full");

    let err = event_put(&store, 10, "evt-1", None, None, "BEGIN:VEVENT\nEND:VEVENT")
        .await
        .unwrap_err();

    expect_server_error(err, "disk full");
}

#[tokio::test]
async fn event_delete_store_error_surfaces_as_server_error() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(EXAMPLE_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("evt-1", "BEGIN:VEVENT\nEND:VEVENT"))
        .delete_event_fails("delete crashed");

    let err = event_delete(&store, 10, "evt-1").await.unwrap_err();

    expect_server_error(err, "delete crashed");
}

// ---------- address book surface ----------

#[tokio::test]
async fn addressbook_home_propfind_ensure_default_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new().ensure_default_fails("ab-bootstrap boom");

    let err = addressbook_home_propfind(&store, EXAMPLE_USER, 0).await.unwrap_err();

    expect_server_error(err, "ab-bootstrap boom");
}

#[tokio::test]
async fn addressbook_home_propfind_list_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .list_books_fails("books unavailable");

    let err = addressbook_home_propfind(&store, EXAMPLE_USER, 1).await.unwrap_err();

    expect_server_error(err, "books unavailable");
}

#[tokio::test]
async fn addressbook_propfind_list_contacts_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .list_contacts_fails("contacts unavailable");

    let err = addressbook_propfind(&store, EXAMPLE_USER, "Friends", 20, 1)
        .await
        .unwrap_err();

    expect_server_error(err, "contacts unavailable");
}

#[tokio::test]
async fn addressbook_report_list_contacts_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .list_contacts_fails("query broke");

    let body = "<CR:addressbook-query xmlns:CR=\"urn:ietf:params:xml:ns:carddav\"/>";
    let err = addressbook_report(&store, EXAMPLE_USER, "Friends", 20, body)
        .await
        .unwrap_err();

    expect_server_error(err, "query broke");
}

#[tokio::test]
async fn contact_get_store_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .get_contact_fails("contact fetch failed");

    let err = contact_get(&store, 20, "ct-1").await.unwrap_err();

    expect_server_error(err, "contact fetch failed");
}

#[tokio::test]
async fn contact_put_etag_lookup_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .contact_etag_fails("etag missing");

    let err = contact_put(
        &store,
        20,
        "ct-1",
        Some("\"deadbeef\""),
        None,
        "BEGIN:VCARD\nEND:VCARD",
    )
    .await
    .unwrap_err();

    expect_server_error(err, "etag missing");
}

#[tokio::test]
async fn contact_put_write_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .put_contact_fails("vcard write failed");

    let err = contact_put(&store, 20, "ct-1", None, None, "BEGIN:VCARD\nEND:VCARD")
        .await
        .unwrap_err();

    expect_server_error(err, "vcard write failed");
}

#[tokio::test]
async fn contact_delete_store_error_surfaces_as_server_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(20, make_contact("ct-1", "BEGIN:VCARD\nEND:VCARD"))
        .delete_contact_fails("delete failed");

    let err = contact_delete(&store, 20, "ct-1").await.unwrap_err();

    expect_server_error(err, "delete failed");
}
