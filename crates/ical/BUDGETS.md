# mailrs-ical performance budgets

Enforced by `tests/perf_gate.rs`. Run `cargo test -p mailrs-ical --test perf_gate`.

| Path | Budget | Observed P95 (dev) | Headroom |
| --- | ---: | ---: | ---: |
| `parse_invite` (Outlook-shaped complex VEVENT) | 1 ms | ~50 µs | ~20× |
| round-trip parse + serialize (complex) | 2 ms | ~100 µs | ~20× |

The "complex" fixture is an Outlook-emitted invite with VTIMEZONE, RRULE,
EXDATE, multiple ATTENDEEs — representative of the upper-middle of real
calendar invite complexity.

Methodology + re-measurement protocol identical to `crates/jmap/BUDGETS.md`.
