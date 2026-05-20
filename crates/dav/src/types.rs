//! Minimal CalDAV / CardDAV-facing data types.
//!
//! These are intentionally decoupled from any backing store (PostgreSQL, SQLite,
//! filesystem, etc.). A [`CalendarStore`](crate::store::CalendarStore) or
//! [`AddressBookStore`](crate::store::AddressBookStore) implementation is
//! responsible for mapping its own representation into these shapes.

use chrono::{DateTime, Utc};

/// A CalDAV calendar collection.
///
/// The `id` is the store's native primary key; the `name` is what gets exposed
/// as the URL path segment and `<D:displayname>` in PROPFIND output.
#[derive(Debug, Clone)]
pub struct Calendar {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub description: String,
}

/// A single VEVENT (or other VCOMPONENT) stored under a calendar.
///
/// The raw `icalendar` text is kept as the canonical form; structured fields
/// (`summary`, `dtstart`, `dtend`) are projections used for query/reporting
/// performance and may be re-derived from the raw text by the store.
#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub etag: String,
    pub icalendar: String,
    pub summary: String,
    pub dtstart: Option<DateTime<Utc>>,
    pub dtend: Option<DateTime<Utc>>,
}

/// A CardDAV address book collection.
#[derive(Debug, Clone)]
pub struct AddressBook {
    pub id: i64,
    pub name: String,
    pub description: String,
}

/// A single vCard stored under an address book.
#[derive(Debug, Clone)]
pub struct Contact {
    pub uid: String,
    pub etag: String,
    pub vcard: String,
    pub fn_name: String,
    pub email: String,
}

/// Result of a successful PUT (event or contact).
#[derive(Debug, Clone)]
pub struct PutResult {
    /// `true` when the resource didn't exist before this PUT.
    pub created: bool,
    pub etag: String,
}
