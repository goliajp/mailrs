//! `/dav/` principal PROPFIND — the entry point CalDAV / CardDAV clients hit
//! after the `/.well-known/{caldav,carddav}` redirect.
//!
//! Returns the canonical four discovery properties:
//! - `current-user-principal` (RFC 5397)
//! - `calendar-home-set` (RFC 4791 §6.2.1)
//! - `addressbook-home-set` (RFC 6352 §7.1.1)
//! - `principal-URL` + `supported-report-set` (for client compatibility)

use crate::error::DavError;
use crate::xml::{DavResponse, multistatus, xml_escape};

/// Build the principal PROPFIND response body for `user`.
///
/// `request_body` is inspected verbatim to decide which optional properties
/// to include — this matches the behaviour of clients that send a precise
/// `<D:prop>` list versus those that send an empty body to mean "give me
/// everything you've got".
pub fn principal_propfind(user: &str, request_body: &str) -> Result<DavResponse, DavError> {
    let cal_home = format!("/dav/calendars/{user}/");
    let card_home = format!("/dav/contacts/{user}/");

    let wants_current_user_principal = request_body.contains("current-user-principal");
    let wants_resourcetype = request_body.contains("resourcetype");
    let wants_displayname = request_body.contains("displayname");
    let empty = request_body.is_empty();

    let mut props = String::new();
    if wants_current_user_principal || empty {
        props.push_str(
            "<D:current-user-principal><D:href>/dav/</D:href></D:current-user-principal>\n",
        );
    }
    if wants_resourcetype || empty {
        props.push_str("<D:resourcetype><D:collection/></D:resourcetype>\n");
    }
    if wants_displayname || empty {
        props.push_str(&format!(
            "<D:displayname>{}</D:displayname>\n",
            xml_escape(user)
        ));
    }
    props.push_str(&format!(
        "<C:calendar-home-set><D:href>{cal_home}</D:href></C:calendar-home-set>\n\
         <CR:addressbook-home-set><D:href>{card_home}</D:href></CR:addressbook-home-set>\n"
    ));
    props.push_str("<D:principal-URL><D:href>/dav/</D:href></D:principal-URL>\n");
    props.push_str(
        "<D:supported-report-set>\
         <D:supported-report><D:report><C:calendar-multiget/></D:report></D:supported-report>\
         <D:supported-report><D:report><C:calendar-query/></D:report></D:supported-report>\
         <D:supported-report><D:report><CR:addressbook-multiget/></D:report></D:supported-report>\
         <D:supported-report><D:report><CR:addressbook-query/></D:report></D:supported-report>\
         </D:supported-report-set>\n",
    );

    let inner = format!(
        "<D:response>\n\
         <D:href>/dav/</D:href>\n\
         <D:propstat>\n<D:prop>\n{props}</D:prop>\n\
         <D:status>HTTP/1.1 200 OK</D:status>\n\
         </D:propstat>\n\
         </D:response>"
    );
    Ok(multistatus(&inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_includes_calendar_and_addressbook_home_sets() {
        let resp = principal_propfind("alice@example.com", "").unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("<C:calendar-home-set>"));
        assert!(body.contains("/dav/calendars/alice@example.com/"));
        assert!(body.contains("<CR:addressbook-home-set>"));
        assert!(body.contains("/dav/contacts/alice@example.com/"));
    }

    #[test]
    fn principal_empty_body_returns_all_props() {
        let resp = principal_propfind("u", "").unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("current-user-principal"));
        assert!(body.contains("resourcetype"));
        assert!(body.contains("displayname"));
    }

    #[test]
    fn principal_specific_prop_request_returns_only_requested() {
        let req = "<D:propfind><D:prop><D:current-user-principal/></D:prop></D:propfind>";
        let resp = principal_propfind("u", req).unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("current-user-principal"));
        // displayname not requested → absent
        assert!(!body.contains("<D:displayname>"));
    }

    #[test]
    fn principal_xml_escapes_user_in_displayname() {
        let resp = principal_propfind("a<b>c", "").unwrap();
        let body = String::from_utf8(resp.body).unwrap();
        assert!(body.contains("a&lt;b&gt;c"));
    }
}
