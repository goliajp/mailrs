# mailrs-mailbox performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-mailbox --test perf_gate`.

**Scope: pure-algorithm helpers + the in-memory store predicate.** PG-bound
operations are not gated — their cost is dominated by network and DB
latency, not the in-process logic, so a wall-clock budget would catch the
wrong thing.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `extract_message_id` (long headers) | 100 µs | ~3 µs | ~30× |
| `extract_in_reply_to` (long headers) | 100 µs | ~3 µs | ~30× |
| `normalize_message_id` | 20 µs | ~200 ns | ~100× |
| `resolve_thread_id` (known parent) | 50 µs | ~500 ns | ~100× |
| `maildir_flags_to_bitmask` (×100) | 120 µs | ~6 µs | ~20× |
| `bitmask_to_maildir_flags` (×100) | 200 µs | ~9 µs | ~22× |
| `InsertMessage::clone` (×100) | 30 µs | ~1 µs | ~30× |
| `InMemoryMailboxStore::query_messages` (200 msgs) | 1.5 ms | ~70 µs | ~20× |

"Long headers" fixture mirrors a realistic AOL-mailer-style message with
DKIM signature, References chain, MIME boundary header lines.

The three batched gates (×100) run their hot path 100 times per sample
because one call sits below the timer floor (~40 ns). Order-of-magnitude
regressions — accidental per-call allocations, switching `&str` fields to
`String`, replacing the linear filter with an O(n²) variant — will trip
the gates well before they reach production.

`query_messages` is benched against [`InMemoryMailboxStore`] (the test
fixture), not PG: it isolates the predicate cost (lowercase + contains
across three string fields per row) plus sort + paginate, which is the
shape any real store impl shares.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
