#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! # mailrs-jmap
//!
//! Server-side JMAP (RFC 8620 + RFC 8621) dispatcher and method handlers,
//! decoupled from any specific web framework or backing store.
//!
//! Plug your store in by implementing [`MailStore`] (async, object-safe), then
//! call [`dispatch_request`] / [`dispatch_method`] from your HTTP handler.
//!
//! ## What's covered
//!
//! - `Mailbox/get`, `Mailbox/query` (RFC 8621 §2)
//! - `Email/get`, `Email/query`, `Email/set` (RFC 8621 §4)
//! - `Thread/get` (RFC 8621 §3)
//! - `EmailSubmission/set` (RFC 8621 §7) — create only
//! - Method back-references (RFC 8620 §3.7)
//!
//! ## What's not
//!
//! - JMAP push (event source / WebSocket) — the wire format is up to you
//!   because event sourcing is so deeply coupled to your runtime; this crate
//!   stays out of it.
//! - JMAP-Contacts, JMAP-Calendars — different spec families.
//! - Server-side capabilities object — you choose what to advertise; the
//!   `JMAP_*_CAP` constants give you the canonical URIs.
//!
//! ## Quick start
//!
//! ```no_run
//! use async_trait::async_trait;
//! use mailrs_jmap::{
//!     dispatch_request, JmapRequest, MailStore,
//!     types::{Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult},
//! };
//!
//! struct MyStore;
//!
//! #[async_trait]
//! impl MailStore for MyStore {
//!     async fn list_mailboxes(&self, _user: &str) -> Result<Vec<Mailbox>, mailrs_jmap::store::StoreError> {
//!         Ok(vec![])
//!     }
//!     async fn mailbox_status(&self, _id: i64) -> Result<MailboxCounts, mailrs_jmap::store::StoreError> {
//!         Ok(MailboxCounts::default())
//!     }
//!     async fn list_messages(&self, _: i64, _: u32, _: u32) -> Result<Vec<Message>, mailrs_jmap::store::StoreError> { Ok(vec![]) }
//!     async fn get_message_by_db_id(&self, _: &str, _: i64) -> Result<Option<Message>, mailrs_jmap::store::StoreError> { Ok(None) }
//!     async fn list_thread_messages(&self, _: &str, _: &str) -> Result<Vec<Message>, mailrs_jmap::store::StoreError> { Ok(vec![]) }
//!     async fn update_flags(&self, _: i64, _: u32, _: u32) -> Result<(), mailrs_jmap::store::StoreError> { Ok(()) }
//!     async fn add_flags(&self, _: i64, _: u32, _: u32) -> Result<(), mailrs_jmap::store::StoreError> { Ok(()) }
//!     async fn read_message_raw(&self, _: &Message) -> Option<Vec<u8>> { None }
//!     fn parse_message(&self, _: &[u8]) -> ParsedBody { ParsedBody::default() }
//!     async fn submit_message(&self, _: &str, _: &Message, _: &[u8]) -> SubmissionResult {
//!         SubmissionResult { success: false, message: Some("not implemented".into()) }
//!     }
//! }
//!
//! # async fn run() {
//! let store = MyStore;
//! let req: JmapRequest = serde_json::from_str(r#"{
//!     "using": ["urn:ietf:params:jmap:mail"],
//!     "methodCalls": [["Mailbox/get", {}, "c1"]]
//! }"#).unwrap();
//! let resp = dispatch_request(req, "alice@example.com", &store).await;
//! # let _ = resp;
//! # }
//! ```

pub mod build;
pub mod dispatch;
pub mod error;
pub mod fixtures;
pub mod flags;
pub mod ids;
pub mod methods;
pub mod refs;
pub mod store;
pub mod types;

pub use dispatch::{
    dispatch_method, dispatch_request, JmapRequest, JmapResponse, JMAP_CORE_CAP, JMAP_MAIL_CAP,
    JMAP_SUBMISSION_CAP,
};
pub use error::JmapMethodError;
pub use store::{MailStore, StoreError};
