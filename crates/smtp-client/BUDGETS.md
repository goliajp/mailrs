# mailrs-smtp-client performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-smtp-client --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `parse_response` (10-line EHLO) | 50 µs | ~1 µs | ~50× |
| `dot_stuff` (4 KB body, dots every other line) | 500 µs | ~20 µs | ~25× |
| `sort_mx_records` (n=20) | 20 µs | ~500 ns | ~40× |

Network-bound paths (connection establishment, MX resolution, TLS handshake)
are out of scope — their cost is dominated by remote-server latency.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
