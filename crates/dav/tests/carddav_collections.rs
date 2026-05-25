//! Integration tests for CardDAV collection endpoints: PROPFIND on the
//! address-book home, PROPFIND on a single address book, and REPORT
//! (addressbook-multiget / addressbook-query).

use mailrs_dav::carddav::{addressbook_home_propfind, addressbook_propfind, addressbook_report};
use mailrs_dav::fixtures::{
    EXAMPLE_USER, InMemoryAddressBookStore, body_as_str, header_value, make_book, make_contact,
};

// ---------- addressbook_home_propfind ----------

#[tokio::test]
async fn addressbook_home_propfind_depth_zero_returns_only_home_collection() {
    let store = InMemoryAddressBookStore::new().with_book(EXAMPLE_USER, make_book(1, "Friends"));

    let resp = addressbook_home_propfind(&store, EXAMPLE_USER, 0)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert_eq!(resp.status, 207);
    assert!(body.contains(&format!("<D:href>/dav/contacts/{EXAMPLE_USER}/</D:href>")));
    assert!(body.contains("<D:displayname>Address Books</D:displayname>"));
    assert!(!body.contains("/dav/contacts/alice@example.com/Friends/"));
}

#[tokio::test]
async fn addressbook_home_propfind_depth_one_lists_child_books() {
    let mut book = make_book(1, "Friends");
    book.description = "Personal contacts".into();
    let store = InMemoryAddressBookStore::new().with_book(EXAMPLE_USER, book);

    let resp = addressbook_home_propfind(&store, EXAMPLE_USER, 1)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/contacts/{EXAMPLE_USER}/Friends/")));
    assert!(body.contains("<D:displayname>Friends</D:displayname>"));
    assert!(body.contains("<CR:addressbook/>"));
    assert!(body.contains("<CS:getctag>Personal contacts</CS:getctag>"));
    assert!(body.contains("<D:current-user-privilege-set>"));
}

#[tokio::test]
async fn addressbook_home_propfind_auto_creates_default_when_user_has_none() {
    let store = InMemoryAddressBookStore::new();

    let resp = addressbook_home_propfind(&store, EXAMPLE_USER, 1)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    // The ensure_default_address_book hook fired, so PROPFIND now sees the
    // auto-created "Default" book at Depth 1.
    assert!(body.contains("/dav/contacts/alice@example.com/Default/"));
}

#[tokio::test]
async fn addressbook_home_propfind_only_returns_books_for_requested_user() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(1, "Mine"))
        .with_book("bob@example.com", make_book(2, "Bobs"));

    let resp = addressbook_home_propfind(&store, EXAMPLE_USER, 1)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/contacts/{EXAMPLE_USER}/Mine/")));
    assert!(!body.contains("/Bobs/"));
}

// ---------- addressbook_propfind ----------

#[tokio::test]
async fn addressbook_propfind_depth_zero_returns_collection_only() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(20, make_contact("ct-1", "BEGIN:VCARD\nUID:ct-1\nEND:VCARD"));

    let resp = addressbook_propfind(&store, EXAMPLE_USER, "Friends", 20, 0)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/contacts/{EXAMPLE_USER}/Friends/")));
    assert!(body.contains("<CR:addressbook/>"));
    assert!(!body.contains("ct-1.vcf"));
}

#[tokio::test]
async fn addressbook_propfind_depth_one_lists_contact_etags_without_vcard() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(
            20,
            make_contact("ct-1", "BEGIN:VCARD\nUID:ct-1\nFN:Alice\nEND:VCARD"),
        );

    let resp = addressbook_propfind(&store, EXAMPLE_USER, "Friends", 20, 1)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/contacts/{EXAMPLE_USER}/Friends/ct-1.vcf")));
    assert!(body.contains("<D:getetag>"));
    assert!(body.contains("text/vcard"));
    // contact body NOT included on PROPFIND
    assert!(!body.contains("FN:Alice"));
}

// ---------- addressbook_report ----------

#[tokio::test]
async fn addressbook_report_multiget_filters_by_uid() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(20, make_contact("a", "BEGIN:VCARD\nUID:a\nEND:VCARD"))
        .with_contact(20, make_contact("b", "BEGIN:VCARD\nUID:b\nEND:VCARD"));

    let body = format!(
        "<CR:addressbook-multiget xmlns:CR=\"urn:ietf:params:xml:ns:carddav\">\
         <D:href>/dav/contacts/{EXAMPLE_USER}/Friends/a.vcf</D:href>\
         </CR:addressbook-multiget>"
    );
    let resp = addressbook_report(&store, EXAMPLE_USER, "Friends", 20, &body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("Friends/a.vcf"));
    assert!(text.contains("<CR:address-data>"));
    assert!(text.contains("UID:a"));
    assert!(!text.contains("Friends/b.vcf"));
}

#[tokio::test]
async fn addressbook_report_query_returns_every_contact() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(20, make_contact("a", "BEGIN:VCARD\nUID:a\nEND:VCARD"))
        .with_contact(20, make_contact("b", "BEGIN:VCARD\nUID:b\nEND:VCARD"));

    let body = "<CR:addressbook-query xmlns:CR=\"urn:ietf:params:xml:ns:carddav\"/>";
    let resp = addressbook_report(&store, EXAMPLE_USER, "Friends", 20, body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("a.vcf"));
    assert!(text.contains("b.vcf"));
}

#[tokio::test]
async fn addressbook_report_multiget_with_no_hrefs_returns_empty_multistatus() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(20, make_contact("a", "BEGIN:VCARD\nEND:VCARD"));

    let body = "<CR:addressbook-multiget xmlns:CR=\"urn:ietf:params:xml:ns:carddav\"/>";
    let resp = addressbook_report(&store, EXAMPLE_USER, "Friends", 20, body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert_eq!(resp.status, 207);
    assert!(text.contains("<D:multistatus"));
    assert!(!text.contains("a.vcf"));
}

#[tokio::test]
async fn addressbook_report_escapes_vcard_body_for_xml_safety() {
    let store = InMemoryAddressBookStore::new()
        .with_book(EXAMPLE_USER, make_book(20, "Friends"))
        .with_contact(
            20,
            make_contact("a", "BEGIN:VCARD\nUID:a\nFN:<oops> & more\nEND:VCARD"),
        );

    let body = format!(
        "<CR:addressbook-multiget xmlns:CR=\"urn:ietf:params:xml:ns:carddav\">\
         <D:href>/dav/contacts/{EXAMPLE_USER}/Friends/a.vcf</D:href>\
         </CR:addressbook-multiget>"
    );
    let resp = addressbook_report(&store, EXAMPLE_USER, "Friends", 20, &body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("&lt;oops&gt;"));
    assert!(text.contains("&amp; more"));
    assert!(!text.contains("<oops>"));
}

#[tokio::test]
async fn carddav_collection_uses_multistatus_envelope_with_dav_headers() {
    let store = InMemoryAddressBookStore::new().with_book(EXAMPLE_USER, make_book(20, "Friends"));

    let resp = addressbook_propfind(&store, EXAMPLE_USER, "Friends", 20, 0)
        .await
        .unwrap();

    assert_eq!(resp.status, 207);
    assert_eq!(
        header_value(&resp.headers, "content-type"),
        Some("application/xml; charset=utf-8")
    );
    let dav = header_value(&resp.headers, "dav").unwrap();
    assert!(dav.contains("addressbook"));
}
