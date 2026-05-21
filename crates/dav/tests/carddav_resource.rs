//! Integration tests for CardDAV resource endpoints: GET / PUT / DELETE on
//! `/dav/contacts/{user}/{book}/{uid}.vcf`.


use mailrs_dav::fixtures::{
    InMemoryAddressBookStore, body_as_str, header_value, make_book, make_contact,
};
use mailrs_dav::carddav::{contact_delete, contact_get, contact_put};
use mailrs_dav::error::DavError;
use mailrs_dav::xml::etag_of;

const BOOK_ID: i64 = 20;
const VCARD_V1: &str = "BEGIN:VCARD\nUID:ct-1\nFN:Alice\nEND:VCARD";
const VCARD_V2: &str = "BEGIN:VCARD\nUID:ct-1\nFN:Alice Updated\nEND:VCARD";

fn fixture_with_contact(uid: &str, body: &str) -> InMemoryAddressBookStore {
    InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"))
        .with_contact(BOOK_ID, make_contact(uid, body))
}

// ---------- GET ----------

#[tokio::test]
async fn contact_get_returns_200_with_vcard_body_and_quoted_etag() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    let resp = contact_get(&store, BOOK_ID, "ct-1").await.unwrap();

    assert_eq!(resp.status, 200);
    assert_eq!(
        header_value(&resp.headers, "content-type"),
        Some("text/vcard; charset=utf-8")
    );
    let etag_header = header_value(&resp.headers, "etag").unwrap();
    assert!(etag_header.starts_with('"') && etag_header.ends_with('"'));
    assert_eq!(body_as_str(resp.body), VCARD_V1);
}

#[tokio::test]
async fn contact_get_missing_uid_returns_not_found_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let err = contact_get(&store, BOOK_ID, "missing").await.unwrap_err();

    assert!(matches!(err, DavError::NotFound));
}

// ---------- PUT ----------

#[tokio::test]
async fn contact_put_new_returns_201_with_etag() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let resp = contact_put(&store, BOOK_ID, "ct-1", None, None, VCARD_V1)
        .await
        .unwrap();

    assert_eq!(resp.status, 201);
    let expected = etag_of(VCARD_V1);
    assert_eq!(
        header_value(&resp.headers, "etag"),
        Some(format!("\"{expected}\"").as_str())
    );
}

#[tokio::test]
async fn contact_put_existing_returns_204_with_fresh_etag() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    let resp = contact_put(&store, BOOK_ID, "ct-1", None, None, VCARD_V2)
        .await
        .unwrap();

    assert_eq!(resp.status, 204);
    let expected_new = etag_of(VCARD_V2);
    assert_eq!(
        header_value(&resp.headers, "etag"),
        Some(format!("\"{expected_new}\"").as_str())
    );
}

#[tokio::test]
async fn contact_put_then_get_round_trips_body_unchanged() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let body = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Test\r\nUID:ct-1\r\nEND:VCARD";
    contact_put(&store, BOOK_ID, "ct-1", None, None, body)
        .await
        .unwrap();
    let resp = contact_get(&store, BOOK_ID, "ct-1").await.unwrap();

    assert_eq!(body_as_str(resp.body), body);
}

#[tokio::test]
async fn contact_put_if_match_with_correct_etag_succeeds() {
    let store = fixture_with_contact("ct-1", VCARD_V1);
    let current = etag_of(VCARD_V1);

    let resp = contact_put(
        &store,
        BOOK_ID,
        "ct-1",
        Some(&format!("\"{current}\"")),
        None,
        VCARD_V2,
    )
    .await
    .unwrap();

    assert_eq!(resp.status, 204);
}

#[tokio::test]
async fn contact_put_if_match_with_stale_etag_returns_precondition_failed() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    let err = contact_put(
        &store,
        BOOK_ID,
        "ct-1",
        Some("\"deadbeefdeadbeef\""),
        None,
        VCARD_V2,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    assert_eq!(store.contacts_in(BOOK_ID)[0].vcard, VCARD_V1);
}

#[tokio::test]
async fn contact_put_if_match_on_missing_resource_returns_precondition_failed() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let err = contact_put(
        &store,
        BOOK_ID,
        "missing",
        Some("\"whatever\""),
        None,
        VCARD_V1,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    assert!(store.contacts_in(BOOK_ID).is_empty());
}

#[tokio::test]
async fn contact_put_if_none_match_star_blocks_overwrite_of_existing() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    let err = contact_put(&store, BOOK_ID, "ct-1", None, Some("*"), VCARD_V2)
        .await
        .unwrap_err();

    assert!(matches!(err, DavError::PreconditionFailed));
    assert_eq!(store.contacts_in(BOOK_ID)[0].vcard, VCARD_V1);
}

#[tokio::test]
async fn contact_put_if_none_match_star_allows_create_when_resource_absent() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let resp = contact_put(&store, BOOK_ID, "ct-new", None, Some("*"), VCARD_V1)
        .await
        .unwrap();

    assert_eq!(resp.status, 201);
    assert_eq!(store.contacts_in(BOOK_ID).len(), 1);
}

// ---------- DELETE ----------

#[tokio::test]
async fn contact_delete_existing_returns_204_and_removes_row() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    let resp = contact_delete(&store, BOOK_ID, "ct-1").await.unwrap();

    assert_eq!(resp.status, 204);
    assert!(store.contacts_in(BOOK_ID).is_empty());
}

#[tokio::test]
async fn contact_delete_missing_returns_not_found_error() {
    let store = InMemoryAddressBookStore::new()
        .with_book(mailrs_dav::fixtures::EXAMPLE_USER, make_book(BOOK_ID, "Friends"));

    let err = contact_delete(&store, BOOK_ID, "missing").await.unwrap_err();

    assert!(matches!(err, DavError::NotFound));
}

#[tokio::test]
async fn contact_delete_twice_second_call_returns_not_found() {
    let store = fixture_with_contact("ct-1", VCARD_V1);

    assert_eq!(contact_delete(&store, BOOK_ID, "ct-1").await.unwrap().status, 204);
    assert!(matches!(
        contact_delete(&store, BOOK_ID, "ct-1").await.unwrap_err(),
        DavError::NotFound
    ));
}
