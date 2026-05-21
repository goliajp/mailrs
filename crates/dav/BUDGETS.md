# mailrs-dav performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set with
3-30× headroom over the observed P95 on a dev machine so CI fails on
order-of-magnitude regressions, not micro-noise.

Run `cargo test -p mailrs-dav --test perf_gate` to check. Run
`cargo bench -p mailrs-dav` for the full criterion baseline numbers.

## Path taxonomy

CalDAV / CardDAV request/response is a **warm** path — per-request
latency, not per-frame. Per `rules/rust/patterns.md`: warm budgets sit
≤ 10 ms.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `principal_propfind` | 100 µs | ~5 µs | ~20× | sync, no store; discovery anchor |
| `calendar_propfind_depth_1_50_events` | 5 ms | ~250 µs | ~20× | etag listing for 50 events |
| `calendar_report_multiget_50` | 10 ms | ~600 µs | ~16× | full multistatus with `calendar-data` × 50 — typical heavy payload |

## Methodology

- Each test runs the path 100 times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of these
fire:

- Hot-path code changes — multistatus envelope, etag computation, the
  iCalendar/vCard scrapers, or any handler in `caldav.rs` / `carddav.rs`.
- A new bench shows the observed P95 has shifted by > 2×.
- The CI runner changes class.

Never weaken a budget without recording the new observed P95 and the
reason for the slowdown in the commit message.

## Why no CardDAV gates yet

CardDAV is structurally identical to CalDAV (mirror handlers, same
multistatus + etag patterns). Adding `addressbook_*` gates would catch
the same regressions twice. If we later make the CardDAV path diverge
from CalDAV (e.g., contact-specific REPORT filters), we add gates for
that surface then.

## Adding a new gate

1. Add a criterion bench under `benches/` measuring the path.
2. Run it to get an observed P95.
3. Pick a budget at 3-30× headroom over P95.
4. Add a `#[test]` / `#[tokio::test]` in `tests/perf_gate.rs`.
5. Document the budget in this table.
6. Commit all three together — bench, gate, budget — so the budget's
   derivation is auditable from the commit alone.
