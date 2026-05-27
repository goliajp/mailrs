# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Outbound mail queue primitives for Rust MTAs — DKIM signing, DSN generation, MTA-STS lookup, retry/backoff, MX/DANE-aware delivery, plus a pluggable store trait and a Postgres reference implementation.

Extracted from [mailrs] so any Rust MTA project can reuse the parts that are actually painful to get right: signing forwarded mail with ARC, generating standards-compliant Delivery Status Notifications, looking up MTA-STS policies, computing exponential backoff with jitter, and the long tail of "did this 5xx mean I should give up or try later" logic.

## Highlights

- **Trait-pluggable store** — [`QueueStore`] + [`Notifier`] decouple delivery state from any particular backend. An [`InMemoryQueueStore`] ships in-box for tests + pilots.
- **Postgres reference** — [`PgQueueStore`] + [`RedisNotifier`] (behind the default `pg` feature) target the schema mailrs uses, so a real production-grade queue is one constructor call away.
- **Pure primitives, no DB required** — `dkim_sign`, `dsn`, `mta_sts`, and `retry` are pure logic. Disable the `pg` feature and they all still compile and work.
- **ARC sealing for forwarded mail** — `dkim_sign::arc_seal_message` adds an ARC chain alongside DKIM so downstream filters can trust the forwarding hop ([RFC 8617]).
- **Bundled delivery worker** — [`DeliveryWorker`] runs the poll-and-deliver loop over MX records (with DANE TLSA enforcement via [`mailrs-smtp-client`]). PG-only in v1.0.0; a generic worker over the trait surface is planned for v2.
- **Battle-tested** — extracted from a production Rust mail server.

## Quick start (PG-backed)

```rust,no_run
use mailrs_outbound_queue::{DeliveryWorker, PgQueueStore, QueueStore, WorkerConfig};
use std::sync::Arc;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL")?).await?;

// enqueue via the trait …
let store = Arc::new(PgQueueStore::new(pool.clone())) as Arc<dyn QueueStore>;
let id = store
    .enqueue(
        "sender@example.org", "bob@example.com", "example.com",
        b"Subject: hi\r\n\r\nhello\r\n", None,
        chrono::Utc::now().timestamp(), false,
    )
    .await?;
println!("queued message #{id}");

// … or run the bundled worker (Postgres + Redis-backed) to drain the queue
let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()?.build()?;
let worker = DeliveryWorker::new(WorkerConfig::default(), pool, resolver, "smtp.example.org".into());
let (_tx, rx) = tokio::sync::watch::channel(false);
worker.run(rx).await;
# Ok(()) }
```

See [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs) for a no-DB example that drives the trait surface end-to-end.

## Feature flags

| Feature | Default | What it enables |
|---------|---------|-----------------|
| `pg`    | on      | `PgQueueStore`, `RedisNotifier`, and the bundled `DeliveryWorker`. Pulls in `sqlx` (Postgres) and `redis`. |

Disable `pg` for a trait-only build:

```toml
mailrs-outbound-queue = { version = "1", default-features = false }
```

In that mode you get `QueueStore` + `Notifier` + `InMemoryQueueStore` + `InMemoryNotifier` + the pure primitives (`dkim_sign`, `dsn`, `mta_sts`, `retry`). Write your own worker on top.

## Module overview

| Module       | Always | Notes |
|--------------|--------|-------|
| `store`      | yes    | `QueueStore`, `Notifier`, `InMemoryQueueStore`, `InMemoryNotifier`, `StoreError`. |
| `queue`      | yes    | `QueuedMessage`, `QueueStatus`, `is_hard_bounce`. PG free fns (gated by `pg`). |
| `dkim_sign`  | yes    | RFC 6376 DKIM signing + RFC 8617 ARC sealing. |
| `dsn`        | yes    | RFC 3464 / 6533 Delivery Status Notification generation. |
| `mta_sts`    | yes    | RFC 8461 MTA-STS policy lookup. |
| `retry`      | yes    | Backoff schedule + bounce decision. |
| `pg_store`   | `pg`   | `PgQueueStore` + `RedisNotifier`. |
| `worker`     | `pg`   | `DeliveryWorker` poll-and-deliver loop. |

## Two paths through the API

The crate exposes two parallel public surfaces over the same queue semantics:

- **Trait API** (`QueueStore`, `Notifier`) — the portable surface. Use it when you want to plug a non-PG backend in, or when you want full control over the delivery loop.
- **PG free functions** in `queue::` — convenience for the common case where you already hold a `sqlx::PgPool` and just want `queue::enqueue(pool, ...)`. These back the bundled `DeliveryWorker` and are what mailrs itself uses internally.

Both are stable for v1.x. The v2 plan is to consolidate around the trait surface and ship a generic worker, but no v1 user code is expected to break.

## What this crate does NOT do

- No SMTP _server_ — see [`mailrs-smtp-proto`] for the inbound state machine.
- No DKIM _verification_ — outbound only (signing). Verification lives in [`mail-auth`].
- No SPF / DMARC enforcement on inbound. Those belong upstream of this crate.
- No message storage / threading. See `mailrs-mailbox` / `mailrs-maildir` for those.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-outbound-queue`) |
| **test** | line cov: 68.0% (`cargo llvm-cov -p mailrs-outbound-queue --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 6 gate(s) `perf_gate.rs` |
| **size** | release rlib: 2.8 MB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

[RFC 8617]: https://datatracker.ietf.org/doc/html/rfc8617
[mailrs]: https://github.com/goliajp/mailrs
[`mailrs-smtp-client`]: https://crates.io/crates/mailrs-smtp-client
[`mailrs-smtp-proto`]: https://crates.io/crates/mailrs-smtp-proto
[`mail-auth`]: https://crates.io/crates/mail-auth

## Performance

Criterion benches: `cargo bench -p mailrs-outbound-queue`. Per-bench medians + regression budgets are documented in [`BUDGETS.md`](BUDGETS.md) (this crate) and the workspace [`PERFORMANCE.md`](../../PERFORMANCE.md).
