# mailrs-postmaster performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-postmaster --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `extract_bimi_logo_url` | 20 µs | ~200 ns | ~100× |

Postmaster's surface is mostly resolver-bound (DNS lookups) and isn't
amenable to wall-clock gating. The BIMI-record parser is the only
pure-in-process hot path worth tracking.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
