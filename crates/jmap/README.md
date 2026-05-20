# mailrs-jmap

[![Crates.io](https://img.shields.io/crates/v/mailrs-jmap?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-jmap)
[![docs.rs](https://img.shields.io/docsrs/mailrs-jmap?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-jmap)
[![License](https://img.shields.io/crates/l/mailrs-jmap?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-jmap?style=flat-square)](https://crates.io/crates/mailrs-jmap)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

Server-side **JMAP** (RFC 8620 + RFC 8621) dispatcher and method handlers for Rust mail servers — framework-agnostic, BYO mail store via the `MailStore` trait.

Extracted from [mailrs] so any project that wants to expose a JMAP API can do so without re-implementing the dispatcher, method-call envelope, back-reference resolver, or the per-method shape conversions for `Email` / `Mailbox` / `Thread` / `EmailSubmission`.

This is, at the time of writing, the **only** standalone server-side JMAP library on crates.io.

## Highlights

- **Methods covered** —
  `Mailbox/get`, `Mailbox/query` ·
  `Email/get`, `Email/query`, `Email/set` ·
  `Thread/get` ·
  `EmailSubmission/set` (create only)
- **Method back-references** — `#key: { resultOf, name, path }` resolved before each dispatch (RFC 8620 §3.7).
- **Framework-free** — no axum / actix / tower / hyper. The crate hands you `(method, args, callId) -> (method, result, callId)` and stays out of your HTTP layer.
- **Store-free** — implement [`MailStore`] (8 async methods + one sync parser) once and every method handler works.
- **Standard error envelopes** — [`JmapMethodError`] enum maps to the canonical `{"type": "serverFail", "description": "..."}` shape from RFC 8620 §3.6.2.
- **Pure helpers exposed** — `flags_to_keywords`, `keywords_to_flags`, `parse_email_db_id`, `resolve_references`, `build_email_meta`, `parse_address_list`. Use the dispatcher or grab the pieces.

## Quick start

```rust,no_run
use async_trait::async_trait;
use mailrs_jmap::{
    dispatch_request, JmapRequest, MailStore,
    types::{Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult},
    store::StoreError,
};

struct MyStore;

#[async_trait]
impl MailStore for MyStore {
    async fn list_mailboxes(&self, user: &str) -> Result<Vec<Mailbox>, StoreError> {
        // ... read your store
        Ok(vec![Mailbox { id: 1, name: "INBOX".into() }])
    }

    async fn mailbox_status(&self, _id: i64) -> Result<MailboxCounts, StoreError> {
        Ok(MailboxCounts { total: 10, unread: 3 })
    }

    // ... 7 more methods, see docs.rs/mailrs-jmap
#   async fn list_messages(&self, _: i64, _: u32, _: u32) -> Result<Vec<Message>, StoreError> { Ok(vec![]) }
#   async fn get_message_by_db_id(&self, _: &str, _: i64) -> Result<Option<Message>, StoreError> { Ok(None) }
#   async fn list_thread_messages(&self, _: &str, _: &str) -> Result<Vec<Message>, StoreError> { Ok(vec![]) }
#   async fn update_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> { Ok(()) }
#   async fn add_flags(&self, _: i64, _: u32, _: u32) -> Result<(), StoreError> { Ok(()) }
#   async fn read_message_raw(&self, _: &Message) -> Option<Vec<u8>> { None }
#   fn parse_message(&self, _: &[u8]) -> ParsedBody { ParsedBody::default() }
#   async fn submit_message(&self, _: &str, _: &Message, _: &[u8]) -> SubmissionResult {
#       SubmissionResult { success: false, message: None }
#   }
}

# async fn run() {
let store = MyStore;
let req: JmapRequest = serde_json::from_str(r#"{
    "using": ["urn:ietf:params:jmap:mail"],
    "methodCalls": [
        ["Mailbox/get", {}, "c1"],
        ["Email/query", {"limit": 10}, "c2"]
    ]
}"#).unwrap();

let resp = dispatch_request(req, "alice@example.com", &store).await;
println!("{}", serde_json::to_string_pretty(&resp).unwrap());
# }
```

## How it slots into axum

```rust,ignore
use std::sync::Arc;
use axum::{extract::State, response::IntoResponse, Json};
use mailrs_jmap::{dispatch_request, JmapRequest};

async fn jmap_api(
    State(store): State<Arc<dyn mailrs_jmap::MailStore>>,
    user: AuthUser, // from your auth middleware
    Json(req): Json<JmapRequest>,
) -> impl IntoResponse {
    Json(dispatch_request(req, &user.address, store.as_ref()).await)
}
```

The store impl is yours; mailrs itself wraps `mailrs_mailbox::MailboxStore` in a thin adapter that bridges its row types into [`mailrs_jmap::types`].

## What's intentionally not in this crate

- **The session endpoint** (`/.well-known/jmap`). It's a 30-line JSON blob driven by your hostname, account address, and which capabilities you advertise — there's nothing to share.
- **Push notifications** (EventSource / WebSocket). The wire format is fixed by RFC 8620 §7 but the event-source plumbing is too coupled to your runtime to share cleanly.
- **JMAP-Contacts** / **JMAP-Calendars** — different specs.

## Versioning

`1.0.0` and onward follows semver. The `MailStore` trait surface and the dispatcher signatures are the public API; helper-module internals (e.g. exact JSON shape inside `build::extend_with_body`) may evolve within a minor version.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
