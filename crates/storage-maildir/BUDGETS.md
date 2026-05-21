# mailrs-maildir performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-maildir --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `parse_flags` (all standard) | 5 µs | ~50 ns | ~100× |
| `serialize_flags` (5 flags) | 5 µs | ~50 ns | ~100× |
| `add_flag` (existing) | 5 µs | ~50 ns | ~100× |

Tightest budgets in the workspace — these are leaf string operations
that run per message on flag mutation, so regressions compound fast.

Filesystem operations (entry walking, rename atomicity) are out of
scope: those are I/O-bound and dependent on filesystem layout.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
