#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! # mailrs-dav
//!
//! Server-side **CalDAV** (RFC 4791) and **CardDAV** (RFC 6352) handlers for
//! Rust mail / calendar / contacts servers — framework-agnostic, BYO data
//! layer via the [`CalendarStore`] and [`AddressBookStore`] traits.
//!
//! At the time of writing this is the only standalone CalDAV / CardDAV
//! **server-side** library on crates.io.
//!
//! ## What's covered
//!
//! - **Principal discovery** ([`principal::principal_propfind`]) — PROPFIND
//!   on `/dav/` returns `current-user-principal`, `calendar-home-set`,
//!   `addressbook-home-set`, `principal-URL`, `supported-report-set`.
//! - **CalDAV** ([`caldav`]) — calendar home + collection PROPFIND, REPORT
//!   (calendar-multiget / calendar-query), GET / PUT / DELETE on events,
//!   `If-Match` / `If-None-Match: *` precondition handling.
//! - **CardDAV** ([`carddav`]) — same shape for address books and contacts.
//! - **Pure parsing helpers** ([`parse`]) — iCalendar / vCard field
//!   extraction, `Depth` header, multiget UID extraction.
//! - **OPTIONS / multistatus / etag** ([`xml`]).
//!
//! ## What's not
//!
//! - **Calendar-query time-range filters** (RFC 4791 §9.7). Most clients work
//!   fine with the "return everything" form this crate emits; if you need
//!   filtered queries, layer them on top of [`crate::store::CalendarStore::list_events`].
//! - **Free/busy reports** (RFC 4791 §7.10) — separate spec, not implemented.
//! - **VTIMEZONE / scheduling extensions** (RFC 6638) — out of scope; the
//!   raw icalendar body round-trips intact, so clients that emit VTIMEZONE
//!   can still GET back what they PUT.
//! - **ACL / privilege management** (RFC 3744). The handlers emit a fixed
//!   `<D:all/>` privilege set for the authenticated owner.
//! - **HTTP auth, routing, framework integration.** This crate takes a
//!   resolved `user` and `calendar_id` / `book_id`; the auth and route
//!   parsing live in your server-side wrapper.
//!
//! ## Quick start
//!
//! ```no_run
//! use async_trait::async_trait;
//! use mailrs_dav::{
//!     caldav, carddav,
//!     store::{CalendarStore, AddressBookStore, StoreError},
//!     types::{Calendar, Event, AddressBook, Contact, PutResult},
//! };
//!
//! struct MyStore;
//!
//! #[async_trait]
//! impl CalendarStore for MyStore {
//!     async fn list_calendars(&self, _user: &str) -> Result<Vec<Calendar>, StoreError> { Ok(vec![]) }
//!     async fn get_calendar(&self, _: &str, _: &str) -> Result<Option<Calendar>, StoreError> { Ok(None) }
//!     async fn list_events(&self, _: i64) -> Result<Vec<Event>, StoreError> { Ok(vec![]) }
//!     async fn get_event(&self, _: i64, _: &str) -> Result<Option<Event>, StoreError> { Ok(None) }
//!     async fn event_etag(&self, _: i64, _: &str) -> Result<Option<String>, StoreError> { Ok(None) }
//!     async fn put_event(&self, _: i64, _: &str, _: &str, etag: &str) -> Result<PutResult, StoreError> {
//!         Ok(PutResult { created: true, etag: etag.into() })
//!     }
//!     async fn delete_event(&self, _: i64, _: &str) -> Result<bool, StoreError> { Ok(false) }
//!     async fn ensure_default_calendar(&self, _: &str) -> Result<(), StoreError> { Ok(()) }
//! }
//!
//! # async fn run() {
//! let store = MyStore;
//! // PROPFIND on a calendar collection (calendar id 42, Depth: 1)
//! let resp = caldav::calendar_propfind(&store, "alice@example.com", "Work", 42, 1)
//!     .await
//!     .unwrap();
//! assert_eq!(resp.status, 207);
//! # }
//! ```
//!
//! ## How it slots into axum
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use axum::http::StatusCode;
//! use mailrs_dav::{caldav, xml::DavResponse};
//!
//! async fn axum_event_get(
//!     // resolved by your auth + routing layer
//!     store: Arc<dyn mailrs_dav::store::CalendarStore>,
//!     calendar_id: i64,
//!     uid: String,
//! ) -> impl axum::response::IntoResponse {
//!     match caldav::event_get(store.as_ref(), calendar_id, &uid).await {
//!         Ok(r) => into_axum(r),
//!         Err(e) => into_axum(e.to_dav_response()),
//!     }
//! }
//!
//! fn into_axum(r: DavResponse) -> axum::response::Response {
//!     let mut builder = axum::http::Response::builder()
//!         .status(StatusCode::from_u16(r.status).unwrap());
//!     for (k, v) in r.headers { builder = builder.header(k, v); }
//!     builder.body(axum::body::Body::from(r.body)).unwrap()
//! }
//! ```

pub mod caldav;
pub mod carddav;
pub mod error;
pub mod fixtures;
pub mod parse;
pub mod principal;
pub mod store;
pub mod types;
pub mod xml;

pub use error::DavError;
pub use store::{AddressBookStore, CalendarStore, StoreError};
pub use types::{AddressBook, Calendar, Contact, Event, PutResult};
pub use xml::{DavResponse, etag_of, multistatus, options_response, xml_escape};
