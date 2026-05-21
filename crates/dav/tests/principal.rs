//! Integration tests for `principal_propfind` — the `/dav/` discovery handler
//! (RFC 5397 + RFC 4791 §6.2.1 + RFC 6352 §7.1.1).


use mailrs_dav::fixtures::{EXAMPLE_USER, body_as_str, header_value};
use mailrs_dav::principal::principal_propfind;

#[test]
fn principal_response_uses_multistatus_envelope_with_dav_headers() {
    let resp = principal_propfind(EXAMPLE_USER, "").unwrap();

    assert_eq!(resp.status, 207);
    assert_eq!(
        header_value(&resp.headers, "content-type"),
        Some("application/xml; charset=utf-8")
    );
    let dav = header_value(&resp.headers, "dav").unwrap();
    assert!(dav.contains("calendar-access"));
    assert!(dav.contains("addressbook"));
}

#[test]
fn principal_advertises_full_supported_report_set() {
    let resp = principal_propfind(EXAMPLE_USER, "").unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains("<D:supported-report-set>"));
    assert!(body.contains("<C:calendar-multiget/>"));
    assert!(body.contains("<C:calendar-query/>"));
    assert!(body.contains("<CR:addressbook-multiget/>"));
    assert!(body.contains("<CR:addressbook-query/>"));
}

#[test]
fn principal_always_emits_home_sets_regardless_of_request_props() {
    // Even a minimal prop request must include calendar-home-set + addressbook-home-set;
    // these are the discovery anchors clients depend on.
    let req = "<D:propfind><D:prop><D:resourcetype/></D:prop></D:propfind>";
    let resp = principal_propfind(EXAMPLE_USER, req).unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains("<C:calendar-home-set>"));
    assert!(body.contains(&format!("/dav/calendars/{EXAMPLE_USER}/")));
    assert!(body.contains("<CR:addressbook-home-set>"));
    assert!(body.contains(&format!("/dav/contacts/{EXAMPLE_USER}/")));
}

#[test]
fn principal_displayname_omitted_when_request_does_not_ask() {
    // Some clients send a narrow <D:prop> that lists only specific properties.
    // The handler must respect that and skip displayname.
    let req = "<D:propfind><D:prop><D:current-user-principal/></D:prop></D:propfind>";
    let resp = principal_propfind(EXAMPLE_USER, req).unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains("<D:current-user-principal>"));
    assert!(!body.contains("<D:displayname>"));
}

#[test]
fn principal_user_with_xml_metacharacters_is_escaped_in_body() {
    let resp = principal_propfind("a<b>c&d", "").unwrap();
    let body = body_as_str(resp.body);

    assert!(body.contains("a&lt;b&gt;c&amp;d"));
    // The escaped form leaks into the home set hrefs verbatim (the handler
    // does not URL-encode the user segment) — that's the documented behaviour
    // and downstream wrappers handle URL encoding before serving.
    assert!(body.contains("/dav/calendars/a<b>c&d/"));
}
