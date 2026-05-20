# mailrs-mailbox

[![Crates.io](https://img.shields.io/crates/v/mailrs-mailbox?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-mailbox)
[![docs.rs](https://img.shields.io/docsrs/mailrs-mailbox?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-mailbox)
[![License](https://img.shields.io/crates/l/mailrs-mailbox?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-mailbox?style=flat-square)](https://crates.io/crates/mailrs-mailbox)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue?style=flat-square)](https://www.rust-lang.org)

PostgreSQL-backed mailbox metadata layer for [mailrs] — message indexing, flag operations, conversation/thread views, search projections, and email-analysis storage.

Extracted from [mailrs] so any Rust project building an IMAP / JMAP / chat-style mail UI can lean on the same store. Pairs with [`mailrs-maildir`] for filesystem delivery: this crate owns the metadata index, maildir owns the bytes.

## Highlights

- **Two views over the same data** — classic IMAP `(mailbox, uid, flags)` and chat-style `(thread, conversation, snoozed/archived)`. Both projections kept in sync within a single PostgreSQL store.
- **Thread reconstruction** — `In-Reply-To` / `References` walk with subject-based fallback, materialized as a `thread_id` column for O(1) conversation lookup.
- **Flag operations** — atomic add/remove/replace, conditional-modseq compare-and-swap, IMAP CONDSTORE bookkeeping.
- **Conversation projections** — `list_conversations` with category / domain filters and pagination; `search_conversations` with full-text indexing.
- **Email analysis storage** — typed columns for category, risk score, summary, entities, embeddings (pgvector); `upsert_email_analysis` is the consumer point for [`mailrs-intelligence`].
- **Contacts / sender feedback** — track inbound/outbound contact history, mutual flags, importance bias.

## Quick start

```rust,no_run
use mailrs_mailbox::MailboxStore;

# async fn run() -> Result<(), sqlx::Error> {
let pool = sqlx::PgPool::connect("postgres://localhost/mailrs").await?;
let store = MailboxStore::new(pool);

store.ensure_default_mailboxes("alice@example.com").await?;
let inbox = store.get_mailbox("alice@example.com", "INBOX").await?.unwrap();
let (exists, recent) = store.mailbox_status(inbox.id).await?;
println!("INBOX: {exists} messages, {recent} recent");
# Ok(())
# }
```

## Schema

This crate expects the mailrs PostgreSQL schema (see `scripts/init-schema.sql` in the [mailrs] repo). It uses runtime sqlx queries (`sqlx::query` / `sqlx::query_as`), not compile-time checked macros — `DATABASE_URL` is not needed at build time.

Typical tables consumed: `mailboxes`, `messages`, `email_analysis`, `contacts`, `signatures`, `sender_feedback`.

## Why not compile-time queries?

Compile-time `sqlx::query!` would couple every build to a running PostgreSQL with the expected schema. mailrs ships as a binary and is built in CI without a database; runtime queries keep the build hermetic and let consumers wire any compatible schema.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

[mailrs]: https://github.com/goliajp/mailrs
[`mailrs-maildir`]: https://crates.io/crates/mailrs-maildir
