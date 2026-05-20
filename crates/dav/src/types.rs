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
    /// Store-native primary key. Surfaces as the FK on every event row.
    pub id: i64,
    /// URL-safe collection name; also returned as `<D:displayname>` in PROPFIND.
    pub name: String,
    /// CSS-style color (e.g. `#ff8800`). Returned as `<apple:calendar-color>`
    /// for client compatibility; empty string means "client picks".
    pub color: String,
    /// Free-form description; returned as `<C:calendar-description>`.
    pub description: String,
}

/// A single VEVENT (or other VCOMPONENT) stored under a calendar.
///
/// The raw `icalendar` text is kept as the canonical form; structured fields
/// (`summary`, `dtstart`, `dtend`) are projections used for query/reporting
/// performance and may be re-derived from the raw text by the store.
#[derive(Debug, Clone)]
pub struct Event {
    /// iCalendar `UID` property — globally unique per RFC 5545 §3.8.4.7.
    /// Doubles as the URL path segment in `/dav/.../events/{uid}.ics`.
    pub uid: String,
    /// Strong validator returned as the `ETag` HTTP header and on
    /// `getetag` PROPFIND queries. Stable for as long as the body bytes are
    /// unchanged.
    pub etag: String,
    /// Verbatim RFC 5545 iCalendar text — the canonical form. PUT bodies
    /// round-trip unchanged through GET so client VTIMEZONE / X-* extensions
    /// survive.
    pub icalendar: String,
    /// Projection of the iCalendar `SUMMARY` property; safe for listing UIs.
    pub summary: String,
    /// Projection of `DTSTART` (UTC-normalised). `None` when the event lacks a
    /// start time (rare; e.g. floating VTODO).
    pub dtstart: Option<DateTime<Utc>>,
    /// Projection of `DTEND` (UTC-normalised). `None` for instantaneous events
    /// or VTODO entries without an end.
    pub dtend: Option<DateTime<Utc>>,
}

/// A CardDAV address book collection.
#[derive(Debug, Clone)]
pub struct AddressBook {
    /// Store-native primary key. Surfaces as the FK on every contact row.
    pub id: i64,
    /// URL-safe collection name; also returned as `<D:displayname>`.
    pub name: String,
    /// Free-form description; returned as `<CR:addressbook-description>`.
    pub description: String,
}

/// A single vCard stored under an address book.
#[derive(Debug, Clone)]
pub struct Contact {
    /// vCard `UID` property — globally unique per RFC 6350 §6.7.6. Also the
    /// URL path segment in `/dav/.../contacts/{uid}.vcf`.
    pub uid: String,
    /// Strong validator returned as `ETag` and on `getetag` PROPFIND queries.
    pub etag: String,
    /// Verbatim RFC 6350 vCard text — the canonical form. PUT bodies round-
    /// trip unchanged through GET so client X-* extensions survive.
    pub vcard: String,
    /// Projection of the vCard `FN` (formatted name) property; safe for
    /// listing UIs. Field name is `fn_name` because `fn` is a Rust keyword.
    pub fn_name: String,
    /// Primary email projection (first `EMAIL` property value, or empty).
    pub email: String,
}

/// Result of a successful PUT (event or contact).
#[derive(Debug, Clone)]
pub struct PutResult {
    /// `true` when the resource didn't exist before this PUT.
    pub created: bool,
    /// The etag the handler will return in the `ETag` response header (RFC
    /// 4791 §5.3.2 / RFC 6352 §6.3.2). Typically the same value the store
    /// recorded — equality is the contract clients depend on.
    pub etag: String,
}
