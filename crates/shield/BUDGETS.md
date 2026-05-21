# mailrs-shield performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-shield --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `reverse_ipv4` | 10 µs | ~100 ns | ~100× |
| `interpret_spamhaus` | 10 µs | ~50 ns | ~200× |
| `evaluate_triplet` (retry case) | 10 µs | ~50 ns | ~200× |
| `triplet_key` | 50 µs | ~500 ns | ~100× |
| `ptr_score_from_names` (match) | 20 µs | ~200 ns | ~100× |

Live DNSBL / PTR lookups are not gated — those are network-bound and
their cost lives outside the in-process pipeline.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
