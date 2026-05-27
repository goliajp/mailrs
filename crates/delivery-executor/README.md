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
| `max_concurrent_flushes` | 2 (1.1+) | how many batches can have fsync in flight simultaneously. `=1` is strictly serial (1.0.0 behavior). `=2` hides fsync wait behind next batch collection — empirically **+8% throughput, -41% p999 tail** on APFS, M-series Mac, 32-conn bench. `>2` typically doesn't help on SSD because the disk serializes durable writes per mount; it just queues more fsyncs. |

```rust,no_run
use mailrs_delivery_executor::DeliveryExecutor;
use std::time::Duration;
let executor = DeliveryExecutor::with_config(/*max_batch=*/ 128, /*max_wait=*/ Duration::from_millis(5));

// Full tuning (1.1+): pipeline 3 flushes for very-high-load
// deployments where you've measured the disk handles parallel
// fsyncs (NVMe, RAID, network FS with concurrent commit).
let executor = DeliveryExecutor::with_full_config(
    /*max_batch=*/ 64,
    /*max_wait=*/ Duration::from_millis(10),
    /*max_concurrent_flushes=*/ 3,
);
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

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-delivery-executor`) |
| **test** | line cov: 96.1% (`cargo llvm-cov -p mailrs-delivery-executor --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 0 gate(s) `perf_gate.rs` |
| **size** | release rlib: 241 KB |
| **fuzz** | ❌ none |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons (from PERFORMANCE.md)

- | SMTP receive throughput, **post DeliveryExecutor** (`mailrs-delivery-executor` 1.0 group-commit, 2026-05-24) | **999 msg/s mean across 3 × 30s × 32 conns** (rounds: 1045 / 972 / 979). **3.4×** vs the immediately-prior 291 msg/s baseline (same hardware, same bench). **P50 32 ms** (vs 105 ms baseline = **3.3× faster**), **P99 41 ms** (vs 163 ms = **4.0× faster**), **P999 76 ms** (vs 199 ms = **2.6× faster**). All four UX axes — throughput, p50, p99, p999 — improve simultaneously; no axis regresses. The win comes from group-commit: 32 concurrent SMTP sessions delivering to the same Maildir path now share a single fsync per batch (max_batch=64, max_wait=10ms) via `mailrs-delivery-executor`'s mpsc → `Maildir::deliver_batch` pipeline, instead of each session driving its own per-message fsync. | `cargo build --profile release-debug -p mailrs-server --bench smtp_load && $CARGO_TARGET_DIR/release-debug/deps/smtp_load-* --duration 30 --conns 32 --warmup 5` |
- | SMTP receive throughput, **post pipelined DeliveryExecutor** (`mailrs-delivery-executor` 1.1, max_concurrent_flushes=2, 2026-05-24) | **1079 msg/s mean across 3 × 30s × 32 conns** (rounds: 1074 / 1073 / 1089). **+8%** vs the 1.0 serial-flush 999 msg/s. **P50 29 ms** (-9%), **P99 36 ms** (-12%), **P999 45 ms (-41%)** — tail latency is the headline win. Mechanism: while batch A's fsync is in flight on a `spawn_blocking` thread, batch B starts collecting concurrently; a `Semaphore`-bounded pipeline of 2 in-flight flushes hides disk-wait behind batch-collection latency without queuing unbounded fsyncs. Cumulative since the perf-axis kickoff (#127): **291 → 1079 msg/s = 3.71× throughput**, **P999 199 → 45 ms = 4.4× faster tail**. | Same reproduce command as the 1.0 row above; binary uses the new published `mailrs-delivery-executor` 1.1 default tuning. |

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.

## Performance

Criterion benches: `cargo bench -p mailrs-delivery-executor`. Per-bench medians + regression budgets are documented in [`BUDGETS.md`](BUDGETS.md) (this crate) and the workspace [`PERFORMANCE.md`](../../PERFORMANCE.md).
