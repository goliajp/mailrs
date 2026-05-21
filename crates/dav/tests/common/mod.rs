//! Shared in-memory [`CalendarStore`] and [`AddressBookStore`] implementations
//! used by every integration test binary in this crate.
//!
//! Tests build a store with the chainable `with_*` setters, hand `&store` to a
//! handler, and (where the handler mutates state) read back via the
//! `events_in` / `contacts_in` helpers. Error injection is per-method, so a
//! single test can isolate the exact code path it cares about.

#![allow(dead_code)]

use std::sync::RwLock;

use async_trait::async_trait;

use mailrs_dav::store::{AddressBookStore, CalendarStore, StoreError};
use mailrs_dav::types::{AddressBook, Calendar, Contact, Event, PutResult};
use mailrs_dav::xml::etag_of;

pub const TEST_USER: &str = "alice@example.com";

// =====================================================================
// Calendar store
// =====================================================================

pub struct InMemoryCalendarStore {
    inner: RwLock<CalInner>,
}

struct CalInner {
    calendars: Vec<(String, Calendar)>, // (owner, Calendar)
    events: Vec<(i64, Event)>,          // (calendar_id, Event)
    default_created_for: Vec<String>,   // owners we auto-created a Default for

    list_calendars_error: Option<String>,
    get_calendar_error: Option<String>,
    list_events_error: Option<String>,
    get_event_error: Option<String>,
    event_etag_error: Option<String>,
    put_event_error: Option<String>,
    delete_event_error: Option<String>,
    ensure_default_error: Option<String>,
}

impl InMemoryCalendarStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(CalInner {
                calendars: Vec::new(),
                events: Vec::new(),
                default_created_for: Vec::new(),
                list_calendars_error: None,
                get_calendar_error: None,
                list_events_error: None,
                get_event_error: None,
                event_etag_error: None,
                put_event_error: None,
                delete_event_error: None,
                ensure_default_error: None,
            }),
        }
    }

    pub fn with_calendar(self, owner: &str, cal: Calendar) -> Self {
        self.inner
            .write()
            .unwrap()
            .calendars
            .push((owner.to_string(), cal));
        self
    }

    pub fn with_event(self, calendar_id: i64, event: Event) -> Self {
        self.inner.write().unwrap().events.push((calendar_id, event));
        self
    }

    pub fn list_calendars_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_calendars_error = Some(msg.to_string());
        self
    }

    pub fn get_calendar_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_calendar_error = Some(msg.to_string());
        self
    }

    pub fn list_events_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_events_error = Some(msg.to_string());
        self
    }

    pub fn get_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_event_error = Some(msg.to_string());
        self
    }

    pub fn event_etag_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().event_etag_error = Some(msg.to_string());
        self
    }

    pub fn put_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().put_event_error = Some(msg.to_string());
        self
    }

    pub fn delete_event_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().delete_event_error = Some(msg.to_string());
        self
    }

    pub fn ensure_default_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().ensure_default_error = Some(msg.to_string());
        self
    }

    pub fn events_in(&self, calendar_id: i64) -> Vec<Event> {
        self.inner
            .read()
            .unwrap()
            .events
            .iter()
            .filter(|(c, _)| *c == calendar_id)
            .map(|(_, e)| e.clone())
            .collect()
    }

    pub fn default_calendar_was_created_for(&self, user: &str) -> bool {
        self.inner
            .read()
            .unwrap()
            .default_created_for
            .iter()
            .any(|u| u == user)
    }
}

impl Default for InMemoryCalendarStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CalendarStore for InMemoryCalendarStore {
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_calendars_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .calendars
            .iter()
            .filter(|(o, _)| o == user)
            .map(|(_, c)| c.clone())
            .collect())
    }

    async fn get_calendar(
        &self,
        user: &str,
        calendar_name: &str,
    ) -> Result<Option<Calendar>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_calendar_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .calendars
            .iter()
            .find(|(o, c)| o == user && c.name == calendar_name)
            .map(|(_, c)| c.clone()))
    }

    async fn list_events(&self, calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_events_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
            .iter()
            .filter(|(c, _)| *c == calendar_id)
            .map(|(_, e)| e.clone())
            .collect())
    }

    async fn get_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<Event>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_event_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
            .iter()
            .find(|(c, e)| *c == calendar_id && e.uid == uid)
            .map(|(_, e)| e.clone()))
    }

    async fn event_etag(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.event_etag_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .events
            .iter()
            .find(|(c, e)| *c == calendar_id && e.uid == uid)
            .map(|(_, e)| e.etag.clone()))
    }

    async fn put_event(
        &self,
        calendar_id: i64,
        uid: &str,
        icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.put_event_error {
            return Err(msg.clone().into());
        }
        let pos = inner
            .events
            .iter()
            .position(|(c, e)| *c == calendar_id && e.uid == uid);
        let created = pos.is_none();
        if let Some(p) = pos {
            inner.events[p].1.icalendar = icalendar.to_string();
            inner.events[p].1.etag = etag.to_string();
        } else {
            inner.events.push((
                calendar_id,
                Event {
                    uid: uid.to_string(),
                    etag: etag.to_string(),
                    icalendar: icalendar.to_string(),
                    summary: String::new(),
                    dtstart: None,
                    dtend: None,
                },
            ));
        }
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_event(
        &self,
        calendar_id: i64,
        uid: &str,
    ) -> Result<bool, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.delete_event_error {
            return Err(msg.clone().into());
        }
        let before = inner.events.len();
        inner
            .events
            .retain(|(c, e)| !(*c == calendar_id && e.uid == uid));
        Ok(inner.events.len() < before)
    }

    async fn ensure_default_calendar(&self, user: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.ensure_default_error {
            return Err(msg.clone().into());
        }
        let has_any = inner.calendars.iter().any(|(o, _)| o == user);
        if !has_any {
            let next_id = (inner.calendars.len() as i64) + 1;
            inner.calendars.push((
                user.to_string(),
                Calendar {
                    id: next_id,
                    name: "Default".to_string(),
                    color: String::new(),
                    description: String::new(),
                },
            ));
            inner.default_created_for.push(user.to_string());
        }
        Ok(())
    }
}

// =====================================================================
// AddressBook store
// =====================================================================

pub struct InMemoryAddressBookStore {
    inner: RwLock<AbInner>,
}

struct AbInner {
    books: Vec<(String, AddressBook)>,
    contacts: Vec<(i64, Contact)>,
    default_created_for: Vec<String>,

    list_books_error: Option<String>,
    get_book_error: Option<String>,
    list_contacts_error: Option<String>,
    get_contact_error: Option<String>,
    contact_etag_error: Option<String>,
    put_contact_error: Option<String>,
    delete_contact_error: Option<String>,
    ensure_default_error: Option<String>,
}

impl InMemoryAddressBookStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(AbInner {
                books: Vec::new(),
                contacts: Vec::new(),
                default_created_for: Vec::new(),
                list_books_error: None,
                get_book_error: None,
                list_contacts_error: None,
                get_contact_error: None,
                contact_etag_error: None,
                put_contact_error: None,
                delete_contact_error: None,
                ensure_default_error: None,
            }),
        }
    }

    pub fn with_book(self, owner: &str, book: AddressBook) -> Self {
        self.inner
            .write()
            .unwrap()
            .books
            .push((owner.to_string(), book));
        self
    }

    pub fn with_contact(self, book_id: i64, contact: Contact) -> Self {
        self.inner.write().unwrap().contacts.push((book_id, contact));
        self
    }

    pub fn list_books_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_books_error = Some(msg.to_string());
        self
    }

    pub fn get_book_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_book_error = Some(msg.to_string());
        self
    }

    pub fn list_contacts_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_contacts_error = Some(msg.to_string());
        self
    }

    pub fn get_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_contact_error = Some(msg.to_string());
        self
    }

    pub fn contact_etag_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().contact_etag_error = Some(msg.to_string());
        self
    }

    pub fn put_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().put_contact_error = Some(msg.to_string());
        self
    }

    pub fn delete_contact_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().delete_contact_error = Some(msg.to_string());
        self
    }

    pub fn ensure_default_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().ensure_default_error = Some(msg.to_string());
        self
    }

    pub fn contacts_in(&self, book_id: i64) -> Vec<Contact> {
        self.inner
            .read()
            .unwrap()
            .contacts
            .iter()
            .filter(|(b, _)| *b == book_id)
            .map(|(_, c)| c.clone())
            .collect()
    }
}

impl Default for InMemoryAddressBookStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AddressBookStore for InMemoryAddressBookStore {
    async fn list_address_books(&self, user: &str) -> Result<Vec<AddressBook>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_books_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .books
            .iter()
            .filter(|(o, _)| o == user)
            .map(|(_, b)| b.clone())
            .collect())
    }

    async fn get_address_book(
        &self,
        user: &str,
        book_name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_book_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .books
            .iter()
            .find(|(o, b)| o == user && b.name == book_name)
            .map(|(_, b)| b.clone()))
    }

    async fn list_contacts(&self, book_id: i64) -> Result<Vec<Contact>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_contacts_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .filter(|(b, _)| *b == book_id)
            .map(|(_, c)| c.clone())
            .collect())
    }

    async fn get_contact(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<Contact>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_contact_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .find(|(b, c)| *b == book_id && c.uid == uid)
            .map(|(_, c)| c.clone()))
    }

    async fn contact_etag(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<Option<String>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.contact_etag_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .contacts
            .iter()
            .find(|(b, c)| *b == book_id && c.uid == uid)
            .map(|(_, c)| c.etag.clone()))
    }

    async fn put_contact(
        &self,
        book_id: i64,
        uid: &str,
        vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.put_contact_error {
            return Err(msg.clone().into());
        }
        let pos = inner
            .contacts
            .iter()
            .position(|(b, c)| *b == book_id && c.uid == uid);
        let created = pos.is_none();
        if let Some(p) = pos {
            inner.contacts[p].1.vcard = vcard.to_string();
            inner.contacts[p].1.etag = etag.to_string();
        } else {
            inner.contacts.push((
                book_id,
                Contact {
                    uid: uid.to_string(),
                    etag: etag.to_string(),
                    vcard: vcard.to_string(),
                    fn_name: String::new(),
                    email: String::new(),
                },
            ));
        }
        Ok(PutResult {
            created,
            etag: etag.to_string(),
        })
    }

    async fn delete_contact(
        &self,
        book_id: i64,
        uid: &str,
    ) -> Result<bool, StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.delete_contact_error {
            return Err(msg.clone().into());
        }
        let before = inner.contacts.len();
        inner
            .contacts
            .retain(|(b, c)| !(*b == book_id && c.uid == uid));
        Ok(inner.contacts.len() < before)
    }

    async fn ensure_default_address_book(&self, user: &str) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.ensure_default_error {
            return Err(msg.clone().into());
        }
        let has = inner.books.iter().any(|(o, _)| o == user);
        if !has {
            let next_id = (inner.books.len() as i64) + 1;
            inner.books.push((
                user.to_string(),
                AddressBook {
                    id: next_id,
                    name: "Default".to_string(),
                    description: String::new(),
                },
            ));
            inner.default_created_for.push(user.to_string());
        }
        Ok(())
    }
}

// =====================================================================
// Convenience constructors
// =====================================================================

pub fn make_calendar(id: i64, name: &str) -> Calendar {
    Calendar {
        id,
        name: name.to_string(),
        color: "#abcdef".to_string(),
        description: format!("calendar {name}"),
    }
}

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

pub fn make_book(id: i64, name: &str) -> AddressBook {
    AddressBook {
        id,
        name: name.to_string(),
        description: format!("address book {name}"),
    }
}

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
pub fn header_value<'a>(
    headers: &'a [(String, String)],
    name: &str,
) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}
