# mailrs-dav

[![Crates.io](https://img.shields.io/crates/v/mailrs-dav?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-dav)
[![docs.rs](https://img.shields.io/docsrs/mailrs-dav?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-dav)
[![License](https://img.shields.io/crates/l/mailrs-dav?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-dav?style=flat-square)](https://crates.io/crates/mailrs-dav)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

Server-side **CalDAV** (RFC 4791) and **CardDAV** (RFC 6352) handlers for Rust mail / calendar / contacts servers — framework-agnostic, BYO data layer via the `CalendarStore` and `AddressBookStore` traits.

Extracted from [mailrs] so any project that wants to expose CalDAV / CardDAV can do so without re-implementing the multistatus / propstat builder, multiget UID extraction, iCalendar / vCard field scrapers, or the per-resource precondition handling (`If-Match`, `If-None-Match: *`).

This is, at the time of writing, the **only** standalone server-side CalDAV / CardDAV library on crates.io.

## Highlights

- **Methods covered** —
  PROPFIND (principal / calendar home / calendar / addressbook home / addressbook) ·
  REPORT (`calendar-multiget`, `calendar-query`, `addressbook-multiget`, `addressbook-query`) ·
  GET / PUT / DELETE on events + contacts ·
  OPTIONS / `Depth` header handling.
- **Preconditions** — `If-Match` (etag must equal current) and `If-None-Match: *` (resource must not exist) honored on PUT.
- **Framework-free** — no axum / actix / tower / hyper. Each handler returns a `DavResponse { status, headers, body }` your server-side adapter translates into your framework's response type.
- **Store-free** — implement `CalendarStore` (8 async methods) and / or `AddressBookStore` (8 async methods); every handler works.
- **Pure helpers exposed** — `xml_escape`, `multistatus`, `etag_of`, `options_response`, `extract_ical_field`, `extract_ical_datetime`, `extract_vcard_field`, `extract_multiget_uids`, `parse_depth`. Use the handlers or grab the pieces.
- **Standard error envelope** — `DavError` enum with `.to_dav_response()` for a 4xx/5xx fallback your server can emit unchanged.

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
    async fn list_calendars(&self, user: &str) -> Result<Vec<Calendar>, StoreError> {
        // ... read your store
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

The store impl is yours; mailrs itself wraps its `sqlx::PgPool` in a thin `DavAdapter` that bridges the schema's row types into `mailrs_dav::types`.

## What's intentionally not in this crate

- **HTTP auth.** Basic / Bearer / OAuth is the wrapper's job — this crate takes a resolved `user` string.
- **Routing / URL parsing.** Handlers take pre-resolved `calendar_id` / `book_id`; the URL → ID lookup is your wrapper's call (and trivial, because the store trait gives you `get_calendar` / `get_address_book`).
- **Calendar-query time-range filters** (RFC 4791 §9.7). Most clients work fine with "return all events"; if you need filtering, layer it on top of `list_events`.
- **Free/busy reports** (RFC 4791 §7.10) and **scheduling extensions** (RFC 6638) — different specs.
- **ACL** (RFC 3744). The handlers emit a fixed `<D:all/>` privilege set for the authenticated owner.
- **MKCALENDAR / MKCOL.** Calendar creation is a higher-level admin concern in most mail servers.

## Versioning

`1.0.0` and onward follows semver. The `CalendarStore` / `AddressBookStore` trait surfaces and the handler signatures are the public API; the exact XML shape inside `multistatus` may evolve within a minor version as long as it stays compliant with the matching RFC.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
