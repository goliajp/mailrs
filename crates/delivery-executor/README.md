# mailrs-delivery-executor

[![Crates.io](https://img.shields.io/crates/v/mailrs-delivery-executor.svg)](https://crates.io/crates/mailrs-delivery-executor)
[![Docs.rs](https://docs.rs/mailrs-delivery-executor/badge.svg)](https://docs.rs/mailrs-delivery-executor)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Group-commit delivery executor on top of
[`mailrs-maildir`](https://crates.io/crates/mailrs-maildir) 1.2's
`deliver_batch`. Accumulates per-path delivery requests from
concurrent async tasks (SMTP / LMTP / IMAP APPEND sessions) and
flushes each path's batch via a **single fsync** instead of
N per-message fsyncs.

Built on `tokio::sync::mpsc` + `tokio::sync::oneshot`. Each
calling session submits a delivery and awaits its own
`oneshot::Receiver` for individual confirmation.

## Why

mailrs-maildir 1.2's `deliver_batch` is **15.27× faster** than
N × `deliver` at N=64 batches on APFS (criterion microbench).
But typical mail receivers have the wrong shape to use it
directly: each SMTP session delivers 1-N recipients, and N is
small. No caller is naturally going to hand a batch of 64 messages
to a single `deliver_batch` call.

This crate is the bridge. The executor task accumulates per-path
requests across concurrent sessions, groups them by destination,
and flushes each group through `deliver_batch`. At saturation,
batches naturally fill to `max_batch=64` and the full microbench
speedup translates to real throughput.

## Quick start

```rust,no_run
use mailrs_delivery_executor::DeliveryExecutor;
use std::sync::Arc;

# async fn run() -> std::io::Result<()> {
let executor = DeliveryExecutor::spawn();

// In your SMTP session handler:
let path = "/var/mail/example.com/alice".to_string();
let body = Arc::new(b"From: a@b\r\n\r\nhello\r\n".to_vec());
let id = executor.deliver(path, body).await?;
println!("delivered: {}", id.0);
# Ok(())
# }
```

## Tuning

| Knob | Default | Trade |
|---|---|---|
| `max_batch` | 64 | matches maildir 1.2 microbench sweet spot. Higher: marginally more throughput, more memory per batch. Lower: less batching benefit. |
| `max_wait` | 10 ms | upper bound on added per-message latency. Lower (1-2ms): latency-sensitive workloads (transactional mail where SMTP `250 OK` feeds an HTTP response). Higher: low-traffic but batch-amortizing. |

```rust,no_run
use mailrs_delivery_executor::DeliveryExecutor;
use std::time::Duration;
let executor = DeliveryExecutor::with_config(/*max_batch=*/ 128, /*max_wait=*/ Duration::from_millis(5));
```

## What it costs

Per-message latency increases by **up to `max_wait`**. With
`max_wait=10ms` and a load of 32 concurrent connections, batches
fill in 1-5ms in practice. Under truly low load (single message
in flight), the executor waits the full `max_wait` before
flushing — that's 10ms tail added to every delivery. The win
appears when load is high enough to fill the batch before the
timeout.

## What this crate does NOT do

- No **SMTP / LMTP protocol** — caller's session driver parses
  incoming mail and passes raw bytes.
- No **storage** beyond Maildir — for IMAP-backed or
  Dovecot-style backends, write your own executor over those
  primitives. The pattern (per-path accumulate + batch flush)
  is portable; this crate is just the Maildir variant.
- No **delivery scheduling** — first-come-first-served per
  path. For priority queues use a different executor.

## License

Apache-2.0 OR MIT.
