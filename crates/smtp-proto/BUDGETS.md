# mailrs-smtp-proto performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-smtp-proto --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `parse_command` (MAIL FROM) | 20 µs | ~200 ns | ~100× |
| `parse_command` (AUTH PLAIN) | 20 µs | ~200 ns | ~100× |
| `is_valid` (typical address) | 10 µs | ~100 ns | ~100× |
| `split_address` (typical) | 10 µs | ~100 ns | ~100× |
| `format_ehlo_response` (6 caps) | 50 µs | ~500 ns | ~100× |

smtp-proto is pure command/response shaping — every path here runs per
SMTP message in the receive pipeline. Order-of-magnitude regression in
any of these would be visible as throughput drop on the inbound side.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
