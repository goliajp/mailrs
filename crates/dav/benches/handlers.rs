//! Handler benchmarks: drive each interesting CalDAV / CardDAV handler against
//! a minimal in-memory `CalendarStore` / `AddressBookStore`. Hot paths are
//! PROPFIND, REPORT (multiget), and PUT.
//!
//! GET and DELETE are intentionally not benched — they're a tight match
//! between the handler and the store call, with no interesting variation to
//! measure.

use std::hint::black_box;

use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};

use mailrs_dav::caldav::{calendar_home_propfind, calendar_propfind, calendar_report, event_put};
use mailrs_dav::carddav::{
    addressbook_home_propfind, addressbook_propfind, addressbook_report, contact_put,
};
use mailrs_dav::store::{AddressBookStore, CalendarStore, StoreError};
use mailrs_dav::types::{AddressBook, Calendar, Contact, Event, PutResult};
use mailrs_dav::xml::etag_of;

const TEST_USER: &str = "alice@example.com";
const CAL_ID: i64 = 10;
const BOOK_ID: i64 = 20;

// =====================================================================
// Calendar bench store — canned data, no error injection.
// =====================================================================

struct BenchCalendarStore {
    calendars: Vec<Calendar>,
    events: Vec<Event>,
}

impl BenchCalendarStore {
    fn new(event_count: usize) -> Self {
        Self {
            calendars: vec![Calendar {
                id: CAL_ID,
                name: "Work".into(),
                color: "#3366cc".into(),
                description: "Work events".into(),
            }],
            events: (0..event_count)
                .map(|i| {
                    let body = format!(
                        "BEGIN:VEVENT\nUID:evt-{i}\nSUMMARY:meeting {i}\nDTSTART:20240101T100000Z\nEND:VEVENT"
                    );
                    Event {
                        uid: format!("evt-{i}"),
                        etag: etag_of(&body),
                        icalendar: body,
                        summary: String::new(),
                        dtstart: None,
                        dtend: None,
                    }
                })
                .collect(),
        }
    }
}

#[async_trait]
impl CalendarStore for BenchCalendarStore {
    async fn list_calendars(&self, _user: &str) -> Result<Vec<Calendar>, StoreError> {
        Ok(self.calendars.clone())
    }
    async fn get_calendar(&self, _user: &str, name: &str) -> Result<Option<Calendar>, StoreError> {
        Ok(self.calendars.iter().find(|c| c.name == name).cloned())
    }
    async fn list_events(&self, _calendar_id: i64) -> Result<Vec<Event>, StoreError> {
        Ok(self.events.clone())
    }
    async fn get_event(&self, _calendar_id: i64, uid: &str) -> Result<Option<Event>, StoreError> {
        Ok(self.events.iter().find(|e| e.uid == uid).cloned())
    }
    async fn event_etag(&self, _calendar_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .events
            .iter()
            .find(|e| e.uid == uid)
            .map(|e| e.etag.clone()))
    }
    async fn put_event(
        &self,
        _calendar_id: i64,
        _uid: &str,
        _icalendar: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        Ok(PutResult {
            created: true,
            etag: etag.to_string(),
        })
    }
    async fn delete_event(&self, _: i64, _: &str) -> Result<bool, StoreError> {
        Ok(true)
    }
    async fn ensure_default_calendar(&self, _: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

// =====================================================================
// AddressBook bench store
// =====================================================================

struct BenchAddressBookStore {
    books: Vec<AddressBook>,
    contacts: Vec<Contact>,
}

impl BenchAddressBookStore {
    fn new(contact_count: usize) -> Self {
        Self {
            books: vec![AddressBook {
                id: BOOK_ID,
                name: "Friends".into(),
                description: "Personal contacts".into(),
            }],
            contacts: (0..contact_count)
                .map(|i| {
                    let body = format!(
                        "BEGIN:VCARD\nVERSION:4.0\nUID:ct-{i}\nFN:Person {i}\nEMAIL:p{i}@example.com\nEND:VCARD"
                    );
                    Contact {
                        uid: format!("ct-{i}"),
                        etag: etag_of(&body),
                        vcard: body,
                        fn_name: String::new(),
                        email: String::new(),
                    }
                })
                .collect(),
        }
    }
}

#[async_trait]
impl AddressBookStore for BenchAddressBookStore {
    async fn list_address_books(&self, _user: &str) -> Result<Vec<AddressBook>, StoreError> {
        Ok(self.books.clone())
    }
    async fn get_address_book(
        &self,
        _user: &str,
        name: &str,
    ) -> Result<Option<AddressBook>, StoreError> {
        Ok(self.books.iter().find(|b| b.name == name).cloned())
    }
    async fn list_contacts(&self, _book_id: i64) -> Result<Vec<Contact>, StoreError> {
        Ok(self.contacts.clone())
    }
    async fn get_contact(&self, _book_id: i64, uid: &str) -> Result<Option<Contact>, StoreError> {
        Ok(self.contacts.iter().find(|c| c.uid == uid).cloned())
    }
    async fn contact_etag(&self, _book_id: i64, uid: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .contacts
            .iter()
            .find(|c| c.uid == uid)
            .map(|c| c.etag.clone()))
    }
    async fn put_contact(
        &self,
        _book_id: i64,
        _uid: &str,
        _vcard: &str,
        etag: &str,
    ) -> Result<PutResult, StoreError> {
        Ok(PutResult {
            created: true,
            etag: etag.to_string(),
        })
    }
    async fn delete_contact(&self, _: i64, _: &str) -> Result<bool, StoreError> {
        Ok(true)
    }
    async fn ensure_default_address_book(&self, _: &str) -> Result<(), StoreError> {
        Ok(())
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

// =====================================================================
// CalDAV handler benches
// =====================================================================

fn bench_caldav_handlers(c: &mut Criterion) {
    let rt = rt();
    // 50 events is realistic for a "Work" calendar.
    let store = BenchCalendarStore::new(50);

    // home_propfind D=1 — first sync hit, lists all calendars.
    c.bench_function("calendar_home_propfind_depth_1", |b| {
        b.iter(|| {
            rt.block_on(async {
                calendar_home_propfind(black_box(&store), black_box(TEST_USER), black_box(1)).await
            })
        })
    });

    // calendar PROPFIND D=1 — etag listing of every event.
    c.bench_function("calendar_propfind_depth_1_50_events", |b| {
        b.iter(|| {
            rt.block_on(async {
                calendar_propfind(
                    black_box(&store),
                    black_box(TEST_USER),
                    black_box("Work"),
                    black_box(CAL_ID),
                    black_box(1),
                )
                .await
            })
        })
    });

    // REPORT multiget — request all 50 events with calendar-data inline.
    let multiget_body = (0..50)
        .map(|i| format!("<D:href>/dav/calendars/{TEST_USER}/Work/evt-{i}.ics</D:href>"))
        .collect::<String>();
    let report_body = format!(
        "<C:calendar-multiget xmlns:C=\"urn:ietf:params:xml:ns:caldav\">{multiget_body}</C:calendar-multiget>"
    );
    c.bench_function("calendar_report_multiget_50", |b| {
        b.iter(|| {
            rt.block_on(async {
                calendar_report(
                    black_box(&store),
                    black_box(TEST_USER),
                    black_box("Work"),
                    black_box(CAL_ID),
                    black_box(&report_body),
                )
                .await
            })
        })
    });

    // event PUT new — etag computation + store write + status mapping.
    let event_body =
        "BEGIN:VEVENT\nUID:evt-new\nSUMMARY:meeting\nDTSTART:20240101T100000Z\nEND:VEVENT";
    c.bench_function("event_put_new_no_preconditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                event_put(
                    black_box(&store),
                    black_box(CAL_ID),
                    black_box("evt-new"),
                    black_box(None),
                    black_box(None),
                    black_box(event_body),
                )
                .await
            })
        })
    });
}

// =====================================================================
// CardDAV handler benches
// =====================================================================

fn bench_carddav_handlers(c: &mut Criterion) {
    let rt = rt();
    let store = BenchAddressBookStore::new(50);

    c.bench_function("addressbook_home_propfind_depth_1", |b| {
        b.iter(|| {
            rt.block_on(async {
                addressbook_home_propfind(black_box(&store), black_box(TEST_USER), black_box(1))
                    .await
            })
        })
    });

    c.bench_function("addressbook_propfind_depth_1_50_contacts", |b| {
        b.iter(|| {
            rt.block_on(async {
                addressbook_propfind(
                    black_box(&store),
                    black_box(TEST_USER),
                    black_box("Friends"),
                    black_box(BOOK_ID),
                    black_box(1),
                )
                .await
            })
        })
    });

    let multiget_body = (0..50)
        .map(|i| format!("<D:href>/dav/contacts/{TEST_USER}/Friends/ct-{i}.vcf</D:href>"))
        .collect::<String>();
    let report_body = format!(
        "<CR:addressbook-multiget xmlns:CR=\"urn:ietf:params:xml:ns:carddav\">{multiget_body}</CR:addressbook-multiget>"
    );
    c.bench_function("addressbook_report_multiget_50", |b| {
        b.iter(|| {
            rt.block_on(async {
                addressbook_report(
                    black_box(&store),
                    black_box(TEST_USER),
                    black_box("Friends"),
                    black_box(BOOK_ID),
                    black_box(&report_body),
                )
                .await
            })
        })
    });

    let contact_body =
        "BEGIN:VCARD\nVERSION:4.0\nUID:ct-new\nFN:New Person\nEMAIL:new@example.com\nEND:VCARD";
    c.bench_function("contact_put_new_no_preconditions", |b| {
        b.iter(|| {
            rt.block_on(async {
                contact_put(
                    black_box(&store),
                    black_box(BOOK_ID),
                    black_box("ct-new"),
                    black_box(None),
                    black_box(None),
                    black_box(contact_body),
                )
                .await
            })
        })
    });
}

criterion_group!(
    handler_benches,
    bench_caldav_handlers,
    bench_carddav_handlers
);
criterion_main!(handler_benches);
