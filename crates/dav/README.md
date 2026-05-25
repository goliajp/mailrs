# mailrs-dav

[![Crates.io](https://img.shields.io/crates/v/mailrs-dav?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dav)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dav?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dav)
[![License](https://img.shields.io/crates/l/mailrs-dav?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-dav?style=flat-square)](https://crates.io/crates/mailrs-dav)

Server-side **CalDAV** ([RFC 4791](https://www.rfc-editor.org/rfc/rfc4791)) and **CardDAV** ([RFC 6352](https://www.rfc-editor.org/rfc/rfc6352)) handlers for Rust mail / calendar / contacts servers — framework-agnostic, BYO data layer via the `CalendarStore` and `AddressBookStore` traits.

Extracted from [mailrs] so any project that wants to expose CalDAV / CardDAV can do so without re-implementing the multistatus / propstat builder, multiget UID extraction, iCalendar / vCard field scrapers, or the per-resource precondition handling (`If-Match`, `If-None-Match: *`).

This is, at the time of writing, the **only** standalone server-side CalDAV / CardDAV library on crates.io.

## Highlights

- **Framework-free** — no axum / actix / tower / hyper. Each handler returns a `DavResponse { status, headers, body }` your server-side adapter translates into your framework's response type.
- **Store-free** — implement [`CalendarStore`](https://docs.rs/mailrs-dav/latest/mailrs_dav/store/trait.CalendarStore.html) (8 async methods) and / or [`AddressBookStore`](https://docs.rs/mailrs-dav/latest/mailrs_dav/store/trait.AddressBookStore.html) (8 async methods); every handler works.
- **Preconditions handled** — `If-Match` (etag must equal current) and `If-None-Match: *` (resource must not exist) honored on PUT per [RFC 4791 §5.3.2](https://www.rfc-editor.org/rfc/rfc4791#section-5.3.2) / [RFC 6352 §6.3.2](https://www.rfc-editor.org/rfc/rfc6352#section-6.3.2).
- **Standard error envelope** — [`DavError`](https://docs.rs/mailrs-dav/latest/mailrs_dav/error/enum.DavError.html) enum with `.to_dav_response()` for a 4xx/5xx fallback your server can emit unchanged.
- **Pure helpers exposed** — `xml_escape`, `multistatus`, `etag_of`, `options_response`, `extract_ical_field`, `extract_ical_datetime`, `extract_vcard_field`, `extract_multiget_uids`, `parse_depth`. Use the handlers or grab the pieces.

## Methods covered (1.0)

| HTTP verb | Resource scope | RFC section | Notes |
| --- | --- | --- | --- |
| OPTIONS | any | [RFC 4918 §9.1](https://www.rfc-editor.org/rfc/rfc4918#section-9.1) | Advertises `DAV: 1, 2, 3, calendar-access, addressbook` + verbs. |
| PROPFIND | `/dav/` | [RFC 5397](https://www.rfc-editor.org/rfc/rfc5397) | `current-user-principal`, `*-home-set`, `principal-URL`, `supported-report-set`. |
| PROPFIND | calendar home / calendar collection | [RFC 4791 §4](https://www.rfc-editor.org/rfc/rfc4791#section-4) | Auto-creates a default calendar on first hit. |
| PROPFIND | addressbook home / addressbook | [RFC 6352 §5](https://www.rfc-editor.org/rfc/rfc6352#section-5) | Auto-creates a default address book on first hit. |
| REPORT | `calendar-multiget` | [RFC 4791 §7.9](https://www.rfc-editor.org/rfc/rfc4791#section-7.9) | UIDs extracted from `<C:href>` children. |
| REPORT | `calendar-query` | [RFC 4791 §7.8](https://www.rfc-editor.org/rfc/rfc4791#section-7.8) | Returns all events; time-range filter is 1.x roadmap (see below). |
| REPORT | `addressbook-multiget` | [RFC 6352 §8.7](https://www.rfc-editor.org/rfc/rfc6352#section-8.7) | UIDs extracted from `<CR:href>` children. |
| REPORT | `addressbook-query` | [RFC 6352 §8.6](https://www.rfc-editor.org/rfc/rfc6352#section-8.6) | Returns all contacts. |
| GET | event / contact | [RFC 4791 §5.3.4](https://www.rfc-editor.org/rfc/rfc4791#section-5.3.4) / [RFC 6352 §6.3.4](https://www.rfc-editor.org/rfc/rfc6352#section-6.3.4) | Verbatim icalendar / vcard body + etag. |
| PUT | event / contact | [RFC 4791 §5.3.2](https://www.rfc-editor.org/rfc/rfc4791#section-5.3.2) / [RFC 6352 §6.3.2](https://www.rfc-editor.org/rfc/rfc6352#section-6.3.2) | 201 on create, 204 on update, 412 on precondition fail. |
| DELETE | event / contact | [RFC 4918 §9.6](https://www.rfc-editor.org/rfc/rfc4918#section-9.6) | 204 on delete, 404 when missing. |

## Quick start

```rust,no_run
use async_trait::async_trait;
use mailrs_dav::{
    caldav,
    store::{CalendarStore, StoreError},
    types::{Calendar, Event, PutResult},
};

struct MyStore;

#[async_trait]
impl CalendarStore for MyStore {
    async fn list_calendars(&self, _user: &str) -> Result<Vec<Calendar>, StoreError> {
        Ok(vec![Calendar {
            id: 1,
            name: "Work".into(),
            color: "#3366cc".into(),
            description: "".into(),
        }])
    }

    // ... 7 more methods, see docs.rs/mailrs-dav
#   async fn get_calendar(&self, _: &str, _: &str) -> Result<Option<Calendar>, StoreError> { Ok(None) }
#   async fn list_events(&self, _: i64) -> Result<Vec<Event>, StoreError> { Ok(vec![]) }
#   async fn get_event(&self, _: i64, _: &str) -> Result<Option<Event>, StoreError> { Ok(None) }
#   async fn event_etag(&self, _: i64, _: &str) -> Result<Option<String>, StoreError> { Ok(None) }
#   async fn put_event(&self, _: i64, _: &str, _: &str, etag: &str) -> Result<PutResult, StoreError> {
#       Ok(PutResult { created: true, etag: etag.into() })
#   }
#   async fn delete_event(&self, _: i64, _: &str) -> Result<bool, StoreError> { Ok(false) }
#   async fn ensure_default_calendar(&self, _: &str) -> Result<(), StoreError> { Ok(()) }
}

# async fn run() {
let store = MyStore;
// PROPFIND on /dav/calendars/alice/Work/ with Depth: 1
let resp = caldav::calendar_propfind(&store, "alice@example.com", "Work", 1, 1)
    .await
    .unwrap();
println!("{} bytes of multistatus XML", resp.body.len());
# }
```

## How it slots into axum

```rust,ignore
use std::sync::Arc;
use axum::{extract::{Path, State}, http::{HeaderMap, Method, StatusCode}, response::Response};
use mailrs_dav::{caldav, parse::parse_depth};

async fn calendar_route(
    method: Method,
    Path((user, calendar)): Path<(String, String)>,
    State(store): State<Arc<dyn mailrs_dav::CalendarStore>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let depth = parse_depth(headers.get("depth").and_then(|v| v.to_str().ok()));
    // ... resolve calendar_id from (user, calendar) via your auth/route layer
    let calendar_id = 1_i64;
    let result = match method.as_str() {
        "PROPFIND" => caldav::calendar_propfind(store.as_ref(), &user, &calendar, calendar_id, depth).await,
        "REPORT" => caldav::calendar_report(store.as_ref(), &user, &calendar, calendar_id, &body).await,
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    let dav_resp = result.unwrap_or_else(|e| e.to_dav_response());
    // ... translate dav_resp into axum::Response
#   axum::http::Response::builder().status(dav_resp.status).body(axum::body::Body::from(dav_resp.body)).unwrap()
}
# use axum::response::IntoResponse;
```

The store impl is yours. The [mailrs] server wraps its `sqlx::PgPool` in a thin `DavAdapter` that bridges the schema's row types into `mailrs_dav::types` — about 250 LOC, worth a read as a reference implementation.

## Tested

`1.0.2` ships **117 tests** — 44 inline unit tests over the pure helpers (iCalendar / vCard scrapers, etag derivation, XML escaping, multistatus envelope, Depth parsing, multiget UID extraction, DavError → DavResponse mapping) and **73 protocol-level integration tests** that drive every handler entry point against in-memory `CalendarStore` / `AddressBookStore` impls:

| Suite | Tests | Surface |
| --- | ---: | --- |
| `tests/principal.rs` | 5 | `principal_propfind` — discovery + supported-report-set + XML escaping |
| `tests/caldav_collections.rs` | 13 | calendar home + collection PROPFIND + REPORT (multiget + query) |
| `tests/caldav_resource.rs` | 15 | event GET / PUT / DELETE — etag preconditions, PUT → GET round-trip |
| `tests/carddav_collections.rs` | 11 | address-book home + collection PROPFIND + REPORT |
| `tests/carddav_resource.rs` | 13 | contact GET / PUT / DELETE |
| `tests/store_error_propagation.rs` | 16 | every fallible store op → `DavError::ServerError` mapping |

The fixtures live at [`mailrs_dav::fixtures::InMemoryCalendarStore`](https://docs.rs/mailrs-dav/latest/mailrs_dav/fixtures/struct.InMemoryCalendarStore.html) and [`mailrs_dav::fixtures::InMemoryAddressBookStore`](https://docs.rs/mailrs-dav/latest/mailrs_dav/fixtures/struct.InMemoryAddressBookStore.html) — same return contracts as a real backend, with per-method error injection so each failure path can be exercised in isolation. As of `1.1.0` it is a `pub` module, so downstream consumers building their own handler tests can use both stores directly without re-implementing them.

## Benchmarks

`1.0.3` ships **21 criterion benchmarks** in two suites — pure-helper microbenchmarks plus async handler benchmarks against inline in-memory stores. Useful both as a regression baseline and as a quick way to compare your own store impl's overhead against the handler floor.

`benches/dav.rs` — sync helpers + composition paths:

- `etag_of` / `xml_escape_*` — SHA-256 digest + entity escaping
- `extract_ical_field` / `extract_ical_datetime` — iCalendar field scrapers
- `extract_multiget_uids_3` — REPORT body UID extraction
- `principal_propfind_*` — discovery handler (sync, no store)
- `multistatus_wrap_*` — envelope wrap on small / 20-entry / 200-entry inner bodies
- `etag_of_*` — etag computation across 60 B / 4 KB payloads

`benches/handlers.rs` — async handlers against minimal in-memory stores:

- `calendar_home_propfind_depth_1` — first-sync hit, lists all calendars
- `calendar_propfind_depth_1_50_events` — etag listing of 50 events
- `calendar_report_multiget_50` — REPORT with `calendar-data` for 50 events
- `event_put_new_no_preconditions` — etag computation + store write
- `addressbook_home_propfind_depth_1` / `addressbook_propfind_depth_1_50_contacts`
- `addressbook_report_multiget_50` — `address-data` inline for 50 contacts
- `contact_put_new_no_preconditions`

GET and DELETE handlers are intentionally not benched — they're a tight match between the handler and a single store call, with no interesting variation to measure. PROPFIND, REPORT, and PUT are where the multistatus / etag composition work happens.

Run with `cargo bench -p mailrs-dav`.

## Roadmap

`1.0` is the minimum viable surface — enough to drive Apple Calendar / Contacts, Thunderbird, DAVx⁵, and other mainstream clients for read + write of events and contacts. Items planned for `1.x`, in rough priority:

- **Calendar-query `time-range` filtering** ([RFC 4791 §9.7](https://www.rfc-editor.org/rfc/rfc4791#section-9.7)). Today `calendar-query` returns the full event list; clients then filter locally. Adding server-side time-range narrows the wire payload for clients that send the filter.
- **MKCALENDAR / MKCOL** ([RFC 4791 §5.3.1](https://www.rfc-editor.org/rfc/rfc4791#section-5.3.1)). Lets clients create new calendars from the UI (currently calendars are created server-side by `ensure_default_calendar` or an admin path).
- **iTIP scheduling extensions** ([RFC 6638](https://www.rfc-editor.org/rfc/rfc6638)). Inbox / outbox collections for invite delivery. The raw iCalendar already round-trips intact, but the protocol-level scheduling collection wiring is out of `1.0`.
- **`getctag` ([CalendarServer ctag extension](https://github.com/apple/ccs-calendarserver/blob/master/doc/Extensions/caldav-ctag.txt))**. A cheap "did anything change in this collection" pre-PROPFIND check for clients that poll.

These will land as additive helpers / new pub fns. The existing trait signatures will not change incompatibly within `1.x`.

## What's intentionally out of scope

- **HTTP auth.** Basic / Bearer / OAuth is the wrapper's job — this crate takes a resolved `user` string.
- **Routing / URL parsing.** Handlers take pre-resolved `calendar_id` / `book_id`; the URL → id lookup is the wrapper's call (and trivial — the store trait gives you `get_calendar` / `get_address_book`).
- **Free/busy reports** ([RFC 4791 §7.10](https://www.rfc-editor.org/rfc/rfc4791#section-7.10)). Separate spec; rarely implemented; clients fall back to fetching events.
- **ACL** ([RFC 3744](https://www.rfc-editor.org/rfc/rfc3744)). The handlers emit a fixed `<D:all/>` privilege set for the authenticated owner; multi-owner / shared-calendar ACLs would need a real authorization model your wrapper owns.

## Versioning

`1.x` follows semver. The public API surface is:

- `CalendarStore` / `AddressBookStore` trait method signatures
- `DavError` enum variants
- `DavResponse` field shapes
- Per-handler `pub fn` signatures in `caldav::*`, `carddav::*`, `principal::*`
- Pure helper signatures in `parse::*` and `xml::*`

The exact XML shape inside `multistatus` may evolve within a minor version as long as it stays compliant with the matching RFC.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-dav`) |
| **test** | line cov: 96.8% (`cargo llvm-cov -p mailrs-dav --summary-only`) |
| **bench** | ✅ 2 file(s) criterion + ✅ 1 gate(s) `perf_gate.rs` |
| **size** | release rlib: 596 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
