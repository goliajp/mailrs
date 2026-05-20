//! [`CalendarStore`] and [`AddressBookStore`] traits: the abstraction layer
//! between DAV handlers and any backing store (PostgreSQL, SQLite, files, etc.).
//!
//! Both traits are async, object-safe, and `Send + Sync` so handler code can
//! take `&dyn CalendarStore` / `&dyn AddressBookStore`.

use async_trait::async_trait;

use crate::types::{AddressBook, Calendar, Contact, Event, PutResult};

/// Opaque store error returned to the dispatcher. Handlers convert it into a
/// [`DavError::ServerError`](crate::error::DavError::ServerError) carrying the
/// `Display` value.
pub type StoreError = Box<dyn std::error::Error + Send + Sync>;

/// Storage operations for CalDAV calendars + events.
#[async_trait]
pub trait CalendarStore: Send + Sync {
    /// List every calendar owned by `user`.
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError>;

    /// Look up a calendar by `user` + URL-decoded `calendar_name`. Returns
    /// `Ok(None)` when the calendar exists but doesn't belong to `user`, or
    /// when no such calendar exists.
    async fn get_calendar(
        &self,
        user: &str,
        calendar_name: &str,
    ) -> Result<Option<Calendar>, StoreError>;

    /// List every event in `calendar_id`. Order is store-defined; handlers do
    /// not rely on ordering.
    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError>;

    /// Look up a single event by `(calendar_id, uid)`.
    async fn get_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<Event>, StoreError>;

    /// Fetch only the etag for `(calendar_id, uid)`. Used by handlers to
    /// perform `If-Match` / `If-None-Match: *` precondition checks without
    /// pulling the full icalendar body.
    async fn event_etag(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Insert or update an event. `icalendar` is the raw RFC 5545 text; `etag`
    /// is a freshly-computed digest the handler wants the store to record.
    ///
    /// Returns a [`PutResult`] indicating whether the row was newly created
    /// (RFC 4791 §5.3.2 distinguishes 201 vs 204).
    async fn put_event(
        &self,
        calendar_id: i64,
        uid: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError>;

    /// Delete an event. Returns `true` when a row was actually removed,
    /// `false` for not-found (handler maps that to 404).
    async fn delete_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<bool, StoreError>;

    /// Idempotent: create a "Default" calendar for `user` if they have none.
    /// Called from PROPFIND on the principal / calendar home so first-login
    /// clients see something to subscribe to.
    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError>;
}

/// Storage operations for CardDAV address books + contacts.
#[async_trait]
pub trait AddressBookStore: Send + Sync {
    /// List every address book owned by `user`.
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError>;

    /// Look up an address book by `user` + URL-decoded `book_name`.
    async fn get_address_book(
        &self,
        user: &str,
        book_name: &str,
    ) -> Result<Option<AddressBook>, StoreError>;

    /// List every contact in `book_id`.
    async fn list_contacts(&self, book_id: i64) -> Result<Vec<Contact>, StoreError>;

    /// Look up a single contact by `(book_id, uid)`.
    async fn get_contact(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<Contact>, StoreError>;

    /// Fetch only the etag for `(book_id, uid)`. Used for precondition checks.
    async fn contact_etag(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Insert or update a contact.
    async fn put_contact(
        &self,
        book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError>;

    /// Delete a contact. Returns `true` on delete, `false` on not-found.
    async fn delete_contact(&self, book_id: i64, uid: &str) -> Result<bool, StoreError>;

    /// Create a "Default" address book for `user` if they have none.
    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError>;
}
