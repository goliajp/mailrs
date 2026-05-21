# mailrs-jmap performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set with
3-30× headroom over the observed P95 on a dev machine so CI fails on
order-of-magnitude regressions, not micro-noise.

Run `cargo test -p mailrs-jmap --test perf_gate` to check. Run
`cargo bench -p mailrs-jmap` for the full criterion baseline numbers.

## Path taxonomy

JMAP request/response is a **warm** path — per-request latency, not
per-frame. Per `rules/rust/patterns.md`: warm budgets sit ≤ 10 ms.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `dispatch_mailbox_get` | 1 ms | ~30 µs | ~30× | 2-mailbox in-memory store |
| `dispatch_email_query` | 2 ms | ~120 µs | ~16× | 10-message in-memory store, default sort |
| `dispatch_request_multi_call_back_ref` | 5 ms | ~300 µs | ~16× | Email/query → Email/get with back-ref, 10-message store |
| `build_email_meta_include_all` | 100 µs | ~3 µs | ~30× | sync composition path, no store |

## Methodology

- Each test runs the path 100 times.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time. Tests must be runnable on
  any reasonable CI executor; we don't pin to high-resolution clocks
  or special hardware.

## When to re-measure

Update the table (and the asserts in `perf_gate.rs`) when any of these
fire:

- Hot-path code changes — the dispatcher core, the meta-composition
  helpers, or the back-reference resolver.
- A new bench shows the observed P95 has shifted by > 2×.
- The CI runner changes class (we move from x86 to arm, etc).

Never weaken a budget without recording the new observed P95 and the
reason for the slowdown in the commit message.

## Adding a new gate

1. Add a criterion bench under `benches/` measuring the path.
2. Run it to get an observed P95.
3. Pick a budget at 3-30× headroom over P95.
4. Add a `#[test]` / `#[tokio::test]` in `tests/perf_gate.rs`.
5. Document the budget in this table.
6. Commit all three together — bench, gate, budget — so the budget's
   derivation is auditable from the commit alone.
