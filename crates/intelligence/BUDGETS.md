# mailrs-intelligence performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-intelligence --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `extract_structured_data` (long HTML w/ two JSON-LD blocks) | 5 ms | ~200 µs | ~25× |
| `calculate_importance` | 50 µs | ~500 ns | ~100× |

`extract_structured_data` is the heavier path — it scans the HTML for
`application/ld+json` script blocks and parses each. Budget assumes ≤ 5
JSON-LD blocks per email, which covers > 99% of real traffic.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
