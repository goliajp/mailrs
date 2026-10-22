# mailrs-outbound-queue

[![Crates.io](https://img.shields.io/crates/v/mailrs-outbound-queue?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-outbound-queue)
[![docs.rs](https://img.shields.io/docsrs/mailrs-outbound-queue?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-outbound-queue)
[![License](https://img.shields.io/crates/l/mailrs-outbound-queue?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/mailrs-outbound-queue?style=flat-square)](https://crates.io/crates/mailrs-outbound-queue)

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md)

Outbound mail queue primitives for Rust MTAs — DKIM signing, DSN generation, MTA-STS lookup, retry/backoff, and an MX/DANE-aware delivery worker over a store you plug in.

Extracted from [mailrs] so any Rust MTA project can reuse the parts that are actually painful to get right: signing forwarded mail with ARC, generating standards-compliant Delivery Status Notifications, looking up MTA-STS policies, computing exponential backoff with jitter, and the long tail of "did this 5xx mean I should give up or try later" logic.

## Highlights

- **Bring your own store** — queue state lives behind the [`QueueStore`] trait and wake-ups behind [`Notifier`]. The crate ships no database backend and pulls in no database driver.
- **A contract you can run** — [`store::contract::check_queue_store`] checks your implementation against every rule the worker relies on (atomic claim, retry and bounce transitions, crash recovery, admin operations, the suppression list). Call it from one test.
- **Bundled delivery worker** — [`DeliveryWorker`] runs the claim-and-deliver loop over any `QueueStore`: MX resolution, DANE / MTA-STS aware STARTTLS via [`mailrs-smtp-client`], DKIM signing, ARC sealing for forwarded mail, retry with backoff, DSNs and hard-bounce suppression.
- **Reference implementations** — [`InMemoryQueueStore`] and [`InMemoryNotifier`] are complete, contract-checked implementations for tests and single-process deployments.
- **Pure primitives** — `dkim_sign`, `dsn`, `mta_sts` and `retry` are usable on their own.

## Quick start

```rust,no_run
use std::sync::Arc;
use mailrs_outbound_queue::{
    DeliveryWorker, InMemoryNotifier, InMemoryQueueStore, Notifier, QueueStore, WorkerConfig,
};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let store = Arc::new(InMemoryQueueStore::new());
let notifier = Arc::new(InMemoryNotifier::new());

let id = store
    .enqueue(
        "sender@example.org", "bob@example.com", "example.com",
        b"Subject: hi\r\n\r\nhello\r\n", None,
        chrono::Utc::now().timestamp(), false,
    )
    .await?;
notifier.notify().await;
println!("queued message #{id}");

let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()?.build()?;
let worker = DeliveryWorker::new(
    WorkerConfig::default(), store, notifier, resolver, "smtp.example.org".into(),
);
let (_stop, stopped) = tokio::sync::watch::channel(false);
worker.run(stopped).await;
# Ok(()) }
```

See [`examples/in_memory_queue.rs`](examples/in_memory_queue.rs) for the store lifecycle without opening sockets.

## Plugging in your own store

1. Implement [`QueueStore`] over your backend. `claim_for_delivery` must be atomic — two concurrent claims never return the same row — so several workers can share one store. In SQL that is one `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED) RETURNING …`.
2. Implement [`Notifier`] over whatever reaches every worker process (a pub/sub channel, a database `LISTEN`), or pass [`NoopNotifier`] and rely on `WorkerConfig::poll_interval_secs`.
3. Check the store:

```rust,ignore
#[tokio::test(flavor = "multi_thread")]
async fn my_store_meets_the_contract() {
    mailrs_outbound_queue::store::contract::check_queue_store(|| async {
        MyStore::connect_to_an_empty_test_database().await
    })
    .await;
}
```

## Module overview

| Module       | Notes |
|--------------|-------|
| `store`      | `QueueStore`, `Notifier`, `NoopNotifier`, `InMemoryQueueStore`, `InMemoryNotifier`, `StoreError`, `contract`. |
| `queue`      | `QueuedMessage`, `QueueStatus`, `is_hard_bounce`. |
| `worker`     | `DeliveryWorker`, `deliver_domain_static`, `try_deliver_via_mx`. |
| `dkim_sign`  | RFC 6376 DKIM signing + RFC 8617 ARC sealing. |
| `dsn`        | RFC 3464 / 6533 Delivery Status Notification generation. |
| `mta_sts`    | RFC 8461 MTA-STS policy lookup. |
| `retry`      | Backoff schedule + bounce decision. |

## What this crate does NOT do

- No SMTP _server_ — see [`mailrs-smtp-proto`] for the inbound state machine.
- No DKIM _verification_ — outbound only (signing). Verification lives in [`mail-auth`].
- No SPF / DMARC enforcement on inbound. Those belong upstream of this crate.
- No message storage / threading. See `mailrs-mailbox` / `mailrs-maildir` for those.

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
