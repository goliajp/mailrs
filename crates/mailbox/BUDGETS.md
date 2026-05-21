# mailrs-mailbox performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-mailbox --test perf_gate`.

**Scope: pure-algorithm helpers only.** PG-bound operations are not
gated — their cost is dominated by network and DB latency, not the
in-process logic, so a wall-clock budget would catch the wrong thing.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `extract_message_id` (long headers) | 100 µs | ~3 µs | ~30× |
| `extract_in_reply_to` (long headers) | 100 µs | ~3 µs | ~30× |
| `normalize_message_id` | 20 µs | ~200 ns | ~100× |
| `resolve_thread_id` (known parent) | 50 µs | ~500 ns | ~100× |

"Long headers" fixture mirrors a realistic AOL-mailer-style message
with DKIM signature, References chain, MIME boundary header lines.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
