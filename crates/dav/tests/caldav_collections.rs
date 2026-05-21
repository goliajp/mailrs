//! Integration tests for CalDAV collection endpoints: PROPFIND on the home
//! collection, PROPFIND on a single calendar, and REPORT
//! (calendar-multiget / calendar-query).

mod common;

use common::{
    InMemoryCalendarStore, TEST_USER, body_as_str, header_value, make_calendar, make_event,
};
use mailrs_dav::caldav::{calendar_home_propfind, calendar_propfind, calendar_report};

// ---------- calendar_home_propfind ----------

#[tokio::test]
async fn calendar_home_propfind_depth_zero_returns_only_home_collection() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(1, "Work"));

    let resp = calendar_home_propfind(&store, TEST_USER, 0).await.unwrap();
    let body = body_as_str(resp.body);

    assert_eq!(resp.status, 207);
    assert!(body.contains(&format!("<D:href>/dav/calendars/{TEST_USER}/</D:href>")));
    assert!(body.contains("<D:displayname>Calendars</D:displayname>"));
    // child calendars only at Depth: 1+
    assert!(!body.contains("/dav/calendars/alice@example.com/Work/"));
}

#[tokio::test]
async fn calendar_home_propfind_depth_one_lists_child_calendars_with_metadata() {
    let mut cal = make_calendar(1, "Work");
    cal.color = "#ff8800".into();
    cal.description = "Work events".into();
    let store = InMemoryCalendarStore::new().with_calendar(TEST_USER, cal);

    let resp = calendar_home_propfind(&store, TEST_USER, 1).await.unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/calendars/{TEST_USER}/Work/")));
    assert!(body.contains("<D:displayname>Work</D:displayname>"));
    assert!(body.contains("#ff8800"));
    assert!(body.contains("<CS:getctag>Work events</CS:getctag>"));
    assert!(body.contains("<C:supported-calendar-component-set>"));
    assert!(body.contains("<C:comp name=\"VEVENT\"/>"));
    assert!(body.contains("<D:current-user-privilege-set>"));
}

#[tokio::test]
async fn calendar_home_propfind_auto_creates_default_when_user_has_none() {
    let store = InMemoryCalendarStore::new();

    let _ = calendar_home_propfind(&store, TEST_USER, 1).await.unwrap();

    assert!(store.default_calendar_was_created_for(TEST_USER));
}

#[tokio::test]
async fn calendar_home_propfind_url_encodes_calendar_name_segment() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(1, "Work + Play"));

    let resp = calendar_home_propfind(&store, TEST_USER, 1).await.unwrap();
    let body = body_as_str(resp.body);

    // space → %20, + → %2B
    assert!(body.contains("/Work%20%2B%20Play/"));
}

#[tokio::test]
async fn calendar_home_propfind_only_returns_calendars_for_requested_user() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(1, "Mine"))
        .with_calendar("bob@example.com", make_calendar(2, "Bobs"));

    let resp = calendar_home_propfind(&store, TEST_USER, 1).await.unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/calendars/{TEST_USER}/Mine/")));
    assert!(!body.contains("/Bobs/"));
}

// ---------- calendar_propfind ----------

#[tokio::test]
async fn calendar_propfind_depth_zero_returns_only_collection_metadata() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("evt-1", "BEGIN:VEVENT\nUID:evt-1\nEND:VEVENT"));

    let resp = calendar_propfind(&store, TEST_USER, "Work", 10, 0)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/calendars/{TEST_USER}/Work/")));
    assert!(body.contains("<C:calendar/>"));
    assert!(body.contains("<D:displayname>Work</D:displayname>"));
    // events suppressed at Depth: 0
    assert!(!body.contains("evt-1.ics"));
}

#[tokio::test]
async fn calendar_propfind_depth_one_lists_event_etags_without_calendar_data() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("evt-1", "BEGIN:VEVENT\nUID:evt-1\nSUMMARY:lunch\nEND:VEVENT"));

    let resp = calendar_propfind(&store, TEST_USER, "Work", 10, 1)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains(&format!("/dav/calendars/{TEST_USER}/Work/evt-1.ics")));
    assert!(body.contains("<D:getetag>"));
    assert!(body.contains("text/calendar"));
    // body content NOT included on PROPFIND (Depth 1) — REPORT is for that
    assert!(!body.contains("SUMMARY:lunch"));
}

// ---------- calendar_report ----------

#[tokio::test]
async fn calendar_report_multiget_returns_calendar_data_for_requested_uids_only() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT"))
        .with_event(10, make_event("b", "BEGIN:VEVENT\nUID:b\nEND:VEVENT"));

    let body = format!(
        "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
         <D:href>/dav/calendars/{TEST_USER}/Work/a.ics</D:href>\
         </C:calendar-multiget>"
    );
    let resp = calendar_report(&store, TEST_USER, "Work", 10, &body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("Work/a.ics"));
    assert!(text.contains("<C:calendar-data>"));
    assert!(text.contains("UID:a"));
    assert!(!text.contains("Work/b.ics"));
}

#[tokio::test]
async fn calendar_report_query_returns_every_event() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("a", "BEGIN:VEVENT\nUID:a\nEND:VEVENT"))
        .with_event(10, make_event("b", "BEGIN:VEVENT\nUID:b\nEND:VEVENT"));

    let body = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let resp = calendar_report(&store, TEST_USER, "Work", 10, body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("a.ics"));
    assert!(text.contains("b.ics"));
}

#[tokio::test]
async fn calendar_report_multiget_with_no_hrefs_returns_empty_multistatus() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(10, make_event("a", "BEGIN:VEVENT\nEND:VEVENT"));

    // multiget without any <D:href> — handler short-circuits to empty multistatus
    let body = "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let resp = calendar_report(&store, TEST_USER, "Work", 10, body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert_eq!(resp.status, 207);
    assert!(text.contains("<D:multistatus"));
    assert!(!text.contains("a.ics"));
}

#[tokio::test]
async fn calendar_report_on_empty_calendar_returns_empty_multistatus() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"));

    let body = "<C:calendar-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"/>";
    let resp = calendar_report(&store, TEST_USER, "Work", 10, body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert_eq!(resp.status, 207);
    assert!(text.contains("<D:multistatus"));
    assert!(!text.contains("<D:response>"));
}

#[tokio::test]
async fn calendar_report_escapes_calendar_data_for_xml_safety() {
    // Embedded `<` / `&` in the icalendar body MUST be escaped or the
    // multistatus payload becomes invalid XML.
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"))
        .with_event(
            10,
            make_event(
                "a",
                "BEGIN:VEVENT\nUID:a\nSUMMARY:<oops> & more\nEND:VEVENT",
            ),
        );

    let body = format!(
        "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
         <D:href>/dav/calendars/{TEST_USER}/Work/a.ics</D:href>\
         </C:calendar-multiget>"
    );
    let resp = calendar_report(&store, TEST_USER, "Work", 10, &body)
        .await
        .unwrap();
    let text = body_as_str(resp.body);

    assert!(text.contains("&lt;oops&gt;"));
    assert!(text.contains("&amp; more"));
    // raw form must not leak through
    assert!(!text.contains("<oops>"));
}

#[tokio::test]
async fn multistatus_envelope_declares_both_caldav_and_carddav_namespaces() {
    let store = InMemoryCalendarStore::new()
        .with_calendar(TEST_USER, make_calendar(10, "Work"));

    let resp = calendar_propfind(&store, TEST_USER, "Work", 10, 0)
        .await
        .unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains("xmlns:D=\"DAV:\""));
    assert!(body.contains("xmlns:C=\"urn:ietf:params:xml:ns:caldav\""));
    assert!(body.contains("xmlns:CR=\"urn:ietf:params:xml:ns:carddav\""));
    assert_eq!(
        header_value(&resp.headers, "content-type"),
        Some("application/xml; charset=utf-8")
    );
}
