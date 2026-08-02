//! Convenience constructors and the two assertion helpers.

use crate::types::{AddressBook, Calendar, Contact, Event};
use crate::xml::etag_of;

/// Convenience example user used by the constructors in this module. The

// =====================================================================
// Convenience constructors
// =====================================================================

/// Build a [`Calendar`] with sane defaults — given id and name, default color and description.
pub fn make_calendar(id: i64, name: &str) -> Calendar {
    Calendar {
        id,
        name: name.to_string(),
        color: "#abcdef".to_string(),
        description: format!("calendar {name}"),
    }
}

/// Build an [`Event`] from a uid and raw iCalendar text, with etag computed via [`etag_of`].
pub fn make_event(uid: &str, body: &str) -> Event {
    Event {
        uid: uid.to_string(),
        etag: etag_of(body),
        icalendar: body.to_string(),
        summary: String::new(),
        dtstart: None,
        dtend: None,
    }
}

/// Build an [`AddressBook`] with sane defaults.
pub fn make_book(id: i64, name: &str) -> AddressBook {
    AddressBook {
        id,
        name: name.to_string(),
        description: format!("address book {name}"),
    }
}

/// Build a [`Contact`] from a uid and raw vCard text, with etag computed via [`etag_of`].
pub fn make_contact(uid: &str, vcard: &str) -> Contact {
    Contact {
        uid: uid.to_string(),
        etag: etag_of(vcard),
        vcard: vcard.to_string(),
        fn_name: String::new(),
        email: String::new(),
    }
}

/// Read the response body as a UTF-8 string. Convenience for substring
/// assertions on multistatus payloads.
pub fn body_as_str(body: Vec<u8>) -> String {
    String::from_utf8(body).expect("dav body is utf-8")
}

/// Find a header value (case-insensitive name match). Returns `None` when
/// absent.
pub fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
