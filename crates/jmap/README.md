# mailrs-jmap

[![Crates.io](https://img.shields.io/crates/v/mailrs-jmap?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-jmap)
[![docs.rs](https://img.shields.io/docsrs/mailrs-jmap?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-jmap)
[![License](https://img.shields.io/crates/l/mailrs-jmap?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-jmap?style=flat-square)](https://crates.io/crates/mailrs-jmap)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

Server-side **JMAP** ([RFC 8620](https://www.rfc-editor.org/rfc/rfc8620) + [RFC 8621](https://www.rfc-editor.org/rfc/rfc8621)) dispatcher and method handlers for Rust mail servers — framework-agnostic, BYO mail store via the `MailStore` trait.

Extracted from [mailrs] so any project that wants to expose a JMAP API can do so without re-implementing the dispatcher, method-call envelope, back-reference resolver, or the per-method shape conversions for `Email` / `Mailbox` / `Thread` / `EmailSubmission`.

This is, at the time of writing, the **only** standalone server-side JMAP library on crates.io.

## Highlights

- **Framework-free** — no axum / actix / tower / hyper. The crate hands you `(method, args, callId) → (method, result, callId)` and stays out of your HTTP layer.
- **Store-free** — implement [`MailStore`](https://docs.rs/mailrs-jmap/latest/mailrs_jmap/store/trait.MailStore.html) (9 async methods + 1 sync parser) once and every method handler works.
- **Method back-references** — `#key: { resultOf, name, path }` resolved before each dispatch ([RFC 8620 §3.7](https://www.rfc-editor.org/rfc/rfc8620#section-3.7)). One round-trip for `Email/query` → `Email/get`.
- **Standard error envelopes** — [`JmapMethodError`](https://docs.rs/mailrs-jmap/latest/mailrs_jmap/error/enum.JmapMethodError.html) maps to the canonical `{"type": "serverFail", "description": "..."}` shape from [RFC 8620 §3.6.2](https://www.rfc-editor.org/rfc/rfc8620#section-3.6.2).
- **Pure helpers exposed** — `flags_to_keywords`, `keywords_to_flags`, `parse_email_db_id`, `resolve_references`, `build_email_meta`, `parse_address_list`. Use the dispatcher or grab the pieces.

## Methods covered (1.0)

| Method | RFC section | Notes |
| --- | --- | --- |
| `Mailbox/get` | [8621 §2.4](https://www.rfc-editor.org/rfc/rfc8621#section-2.4) | All standard properties; role inferred from name (INBOX/Sent/Drafts/Trash). |
| `Mailbox/query` | [8621 §2.5](https://www.rfc-editor.org/rfc/rfc8621#section-2.5) | Unsorted, unfiltered — full list. |
| `Email/get` | [8621 §4.4](https://www.rfc-editor.org/rfc/rfc8621#section-4.4) | Header + body + attachments; respects `properties` selector to skip disk reads. |
| `Email/query` | [8621 §4.5](https://www.rfc-editor.org/rfc/rfc8621#section-4.5) | `inMailbox` filter, `limit` + `position`. |
| `Email/set` | [8621 §4.6](https://www.rfc-editor.org/rfc/rfc8621#section-4.6) | `update` (keywords / mailboxIds) + `destroy`. `create` is rejected as `forbidden` — use `Email/import` (1.1, roadmap). |
| `Thread/get` | [8621 §3.4](https://www.rfc-editor.org/rfc/rfc8621#section-3.4) | Returns `emailIds` in chronological order. |
| `EmailSubmission/set` | [8621 §7.5](https://www.rfc-editor.org/rfc/rfc8621#section-7.5) | `create` only — submits a previously-stored draft via your store's outbound path. |

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
    async fn list_mailboxes(&self, _user: &str) -> Result<Vec<Mailbox>, StoreError> {
        Ok(vec![Mailbox { id: 1, name: "INBOX".into() }])
    }

    async fn mailbox_status(&self, _id: i64) -> Result<MailboxCounts, StoreError> {
        Ok(MailboxCounts { total: 10, unread: 3 })
    }

    // ... 8 more methods, see docs.rs/mailrs-jmap
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
    user: AuthUser, // resolved by your auth middleware
    Json(req): Json<JmapRequest>,
) -> impl IntoResponse {
    Json(dispatch_request(req, &user.address, store.as_ref()).await)
}
```

The store impl is yours. The [mailrs] server uses a thin adapter that bridges its PostgreSQL/Maildir row types into the JMAP shapes in [`mailrs_jmap::types`](https://docs.rs/mailrs-jmap/latest/mailrs_jmap/types/index.html) — about 200 LOC, worth a read as a reference implementation.

## Tested

`1.0.2` ships **98 tests** — 36 inline unit tests over the pure helpers (flag bitmask conversions, id parsers, address-list splitter, back-reference resolver, error-envelope shaping) and **62 protocol-level integration tests** that drive every dispatched method through an in-memory `MailStore` and assert on the response JSON:

| Suite | Tests | Surface |
| --- | ---: | --- |
| `tests/mailbox.rs` | 8 | `Mailbox/get` + `Mailbox/query` |
| `tests/email_get.rs` | 7 | `Email/get` — metadata-only, body, attachments, ownership |
| `tests/email_query.rs` | 10 | `Email/query` — filters, sort, pagination, store-error mapping |
| `tests/email_set.rs` | 14 | `Email/set` — full keywords replace, patch dialect, destroy, every error path |
| `tests/thread_get.rs` | 5 | `Thread/get` — ownership filtering, store-error fallback |
| `tests/email_submission.rs` | 11 | `EmailSubmission/set` — success shape, all 5 documented failure modes |
| `tests/dispatch_request.rs` | 7 | envelope shape, ordering, back-reference resolution, unknown method |

The in-memory fixture (`tests/common/mod.rs`) implements the trait faithfully — same return contracts as a real backend, per-method error injection so a single test can isolate a specific failure path. Useful as a reference implementation if you're building a JMAP test harness of your own.

## Roadmap

`1.0` is the minimum viable surface — enough to drive a webmail client with read, search, mark-read, send, and delete. Methods explicitly not yet implemented, in rough priority order for `1.x`:

- `Identity/get`, `Identity/set` — [RFC 8621 §6](https://www.rfc-editor.org/rfc/rfc8621#section-6). Send-as identities.
- `Mailbox/set` — [RFC 8621 §2.5](https://www.rfc-editor.org/rfc/rfc8621#section-2.5). Create / rename / delete mailboxes.
- `Email/import` — [RFC 8621 §4.8](https://www.rfc-editor.org/rfc/rfc8621#section-4.8). Upload .eml.
- `EmailSubmission/get`, `EmailSubmission/query` — track / cancel pending submissions.
- `VacationResponse/get`, `VacationResponse/set` — [RFC 8621 §8](https://www.rfc-editor.org/rfc/rfc8621#section-8). Out-of-office.

These will land as `MailStore` trait extensions (additive — default impls or feature-gated) so existing `1.0` consumers don't break.

## What's intentionally not in this crate

- **The session endpoint** (`/.well-known/jmap`). It's a small JSON blob driven by your hostname, account address, and which capabilities you advertise — there's nothing to share.
- **Push notifications** (EventSource / WebSocket). The wire format is fixed by [RFC 8620 §7](https://www.rfc-editor.org/rfc/rfc8620#section-7) but the event-source plumbing is too coupled to your runtime to share cleanly.
- **JMAP-Contacts** / **JMAP-Calendars** — different specs. See [mailrs-dav](https://crates.io/crates/mailrs-dav) for CalDAV / CardDAV.
- **The HTTP / authentication / routing layer.** That's the framework job; the dispatcher takes a pre-resolved `user` and gives you back the response envelope to serialize however you like.

## Versioning

`1.x` follows semver. The public API surface is:

- `MailStore` trait method signatures
- `JmapMethodError` enum variants
- `JmapRequest` / `JmapResponse` field shapes
- `dispatch_method` / `dispatch_request` signatures
- The `JMAP_*_CAP` capability URI constants

Helper-module internals (e.g. the exact JSON shape `build::extend_with_body` produces, or the per-method handler signatures inside `methods::*`) may evolve within a minor version; consumers should drive through the dispatcher unless they have a reason not to.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
