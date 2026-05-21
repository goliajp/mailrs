# mailrs-mailbox

[![Crates.io](https://img.shields.io/crates/v/mailrs-mailbox?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-mailbox)
[![docs.rs](https://img.shields.io/docsrs/mailrs-mailbox?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-mailbox)
[![License](https://img.shields.io/crates/l/mailrs-mailbox?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-mailbox?style=flat-square)](https://crates.io/crates/mailrs-mailbox)

Mailbox-metadata storage for Rust mail servers — the IMAP/JMAP-shaped
abstraction every project building an inbox needs, plus a PostgreSQL
reference implementation. Extracted from [mailrs] so any IMAP, JMAP, or
chat-style mail UI can lean on the same battle-tested store.

This is, at the time of writing, the **only standalone server-side
mailbox-metadata library on crates.io**: a portable trait covering
mailbox CRUD, message storage, IMAP CONDSTORE flag ops, threading, and
JMAP-shape change tracking — plus an in-memory fixture that doubles as a
test harness and as proof the trait is genuinely abstract.

## Highlights

- **Trait-first** — code against [`MailboxStore`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/store/trait.MailboxStore.html), a 24-method async trait covering the IMAP and JMAP intersection. Swap the backend without changing handler code.
- **Two reference implementations included** —
  [`pg::PgMailboxStore`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/pg/struct.PgMailboxStore.html) (PostgreSQL, the production-tested one) and [`fixtures::InMemoryMailboxStore`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/fixtures/struct.InMemoryMailboxStore.html) (in-process, for tests and the trait-conformance smell test).
- **CONDSTORE built-in** — per-message `modseq`, `store_flags_if_unchanged` compare-and-swap, `messages_changed_since` for IMAP CHANGEDSINCE and JMAP `Email/changes`.
- **Threading helpers** — pure-function `extract_message_id` / `extract_in_reply_to` / `normalize_message_id` / `resolve_thread_id` in [`threading`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/threading/index.html), no I/O.
- **Flag bitmask interop** — `FLAG_*` constants matching [`mailrs-maildir`](https://crates.io/crates/mailrs-maildir) plus `maildir_flags_to_bitmask` / `bitmask_to_maildir_flags` if you're pairing with filesystem delivery.

## Two-tier API: portable trait vs PG-EXT inherent

The crate intentionally exposes two surfaces:

- **`MailboxStore` trait** — 24 methods rooted in IMAP / JMAP primitives.
  This is the portable contract; downstream consumers should program
  against `&dyn MailboxStore`. Trait methods return store-agnostic types
  ([`Mailbox`], [`Message`], [`MailboxStatus`], [`Inserted`], etc).

- **`PgMailboxStore` inherent methods** — the PostgreSQL implementation
  carries additional methods for content projections, contact-tracking,
  semantic search via pgvector, thread-level UI state (pin / archive /
  snooze), and similar product-shape concerns from the parent [mailrs]
  project. These methods are *public* so mailrs can consume them, but
  documented as `PG-EXT` — they are NOT part of the trait contract and
  should not be relied on by store-agnostic code.

The split is the cleanest expression of "open source isn't just stripping
the `pub` keyword off internal code". The trait covers what mail-server
projects actually share. The PG-EXT methods carry mailrs's specific
product surface without contaminating the abstraction.

## Methods covered (1.0)

The `MailboxStore` trait covers 24 operations grouped by concern:

| Group | Methods | Purpose |
| --- | --- | --- |
| Mailbox CRUD | 7 (`create_mailbox`, `delete_mailbox`, `rename_mailbox`, `list_mailboxes`, `get_mailbox`, `get_mailbox_by_id`, `mailbox_status`) | IMAP CREATE/DELETE/LIST/RENAME/STATUS; JMAP `Mailbox/{get,set,query}` |
| Message CRUD | 8 (`insert_message`, `get_message_by_uid`, `get_message`, `find_by_message_id`, `copy_message`, `move_message`, `expunge`, `messages_changed_since`) | IMAP APPEND/FETCH/COPY/MOVE/EXPUNGE/CHANGEDSINCE; JMAP `Email/{get,set,changes}` |
| Flags + CONDSTORE | 4 (`set_flags`, `add_flags`, `remove_flags`, `store_flags_if_unchanged`) | IMAP STORE / STORE.SILENT / UNCHANGEDSINCE compare-and-swap (RFC 7162) |
| Threading | 3 (`thread_id_for_message`, `thread_message_ids`, `thread_references`) | JMAP `Thread/get`; ancestry walk for `inReplyToId` display |
| Query | 1 (`query_messages`) | JMAP `Email/query`-shape filter: mailbox + text + has_keyword + not_keyword + pagination |
| Quota | 1 (`user_storage_bytes`) | per-user byte sum |

Plus pure helpers in [`threading`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/threading/index.html) (Message-ID parsing, thread resolution) and bitmask conversions in [`types`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/types/index.html).

## Quick start

```rust,no_run
use mailrs_mailbox::{MailboxStore, PgMailboxStore};

# async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
let pool = sqlx::PgPool::connect("postgres://localhost/mailrs").await?;
let store = PgMailboxStore::new(pool);

let mb = MailboxStore::create_mailbox(&store, "alice@example.com", "INBOX").await?;
let status = MailboxStore::mailbox_status(&store, mb.id).await?;
println!("INBOX: {} total, {} unread", status.total, status.unread);
# Ok(())
# }
```

For testing without a database, use [`InMemoryMailboxStore`](https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/fixtures/struct.InMemoryMailboxStore.html):

```rust,no_run
use mailrs_mailbox::fixtures::{InMemoryMailboxStore, EXAMPLE_USER};
use mailrs_mailbox::MailboxStore;

# async fn run() {
let store = InMemoryMailboxStore::new();
let inbox = store.create_mailbox(EXAMPLE_USER, "INBOX").await.unwrap();
assert_eq!(inbox.name, "INBOX");
# }
```

## Schema (PG impl)

The PG reference impl expects the mailrs PostgreSQL schema. The
authoritative DDL lives at `scripts/init-schema.sql` in the [mailrs] repo;
the minimum tables `PgMailboxStore` reads from are `mailboxes` and
`messages`. PG-EXT methods additionally touch `email_analysis`,
`contacts`, `sender_feedback`, `snoozed_conversations`.

`sqlx` is used in **runtime-query mode** (`sqlx::query` / `query_as`),
not compile-time-checked macros. No `DATABASE_URL` needed at build time.

If you want a different schema, implement `MailboxStore` against your
own schema and the trait-driven handlers using this crate just work.

## Tested

`1.0.0` ships **111 tests** across 4 layers:

| Layer | Count | Surface |
| --- | ---: | --- |
| `src/*/tests` (inline) | 67 | Pure helpers (threading, flag bitmask conversion, type roundtrips) |
| `tests/trait_contract.rs` | 35 | Every trait method against [`InMemoryMailboxStore`] — the portable contract suite |
| `tests/smoke.rs` | 5 | PG-specific behaviour against a real Postgres 18 + pgvector container (via [testcontainers](https://crates.io/crates/testcontainers)) — schema application, modseq atomicity, sqlx integration |
| `tests/perf_gate.rs` | 4 | Threading-helper regression budgets (see [BUDGETS.md](./BUDGETS.md)) |

Total density: ~31 tests/kloc, in the same band as the published
`mailrs-jmap` (28) and `mailrs-dav` (34).

Run the portable suite (no Docker needed):

```bash
cargo test -p mailrs-mailbox --test trait_contract
```

Run the PG suite (needs Docker):

```bash
cargo test -p mailrs-mailbox --test smoke -- --test-threads=1
```

## Versioning

`1.x` follows semver. The stable public surface:

- `MailboxStore` trait method signatures
- `StoreError` type alias
- All types in `mailrs_mailbox::types` (marked `#[non_exhaustive]` where
  growth is anticipated, so new fields are minor-bump compatible)
- `pg::PgMailboxStore::new` + `pool` accessor
- Pure helpers in `threading::*` and the `FLAG_*` constants

The set of inherent PG-EXT methods on `PgMailboxStore` may grow or
re-shape within `1.x` to track the parent mailrs project's needs.
Trait-first code is unaffected.

## License

Licensed under either [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`Mailbox`]: https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/types/struct.Mailbox.html
[`Message`]: https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/types/struct.Message.html
[`MailboxStatus`]: https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/types/struct.MailboxStatus.html
[`Inserted`]: https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/types/struct.Inserted.html
[`InMemoryMailboxStore`]: https://docs.rs/mailrs-mailbox/latest/mailrs_mailbox/fixtures/struct.InMemoryMailboxStore.html
